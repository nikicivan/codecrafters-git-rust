use crate::{
    git::{
        commits::Commit,
        compression::decompress,
        file_tree::FileTree,
        git_blob::Blob,
        git_object_trait::{GitObject, GitObjectType},
        git_tree::Tree,
    },
    utils::helpers::{from_utf8_with_context, get_object_file_path, parse_with_context},
};
use anyhow::{anyhow, Context, Ok, Result};
use std::{fs, path::Path};
use strum::EnumTryAs;

#[derive(EnumTryAs, Debug)]
pub enum AnyGitObject {
    Blob(Blob),
    Tree(Tree),
    Commit(Commit),
}

impl AnyGitObject {
    pub fn read(sha: &str) -> Result<Self> {
        let path = get_object_file_path(&sha);

        let raw_content =
            fs::read(&path).with_context(|| format!("failed to read object file at {path:?}"))?;

        AnyGitObject::decode(raw_content)
            .with_context(|| format!("failed to parse object file content for {path:?}"))
    }

    pub fn generate<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        if path.is_file() {
            let content =
                fs::read(path).with_context(|| format!("failed to read file at {path:?}"))?;
            Ok(Self::Blob(Blob::new(content)))
        } else if path.is_dir() {
            let file_tree = FileTree::new(path)?;
            Ok(Self::Tree(file_tree.tree_object().with_context(|| {
                format!("failed to generate tree object from {path:?}")
            })?))
        } else {
            Err(anyhow!(
                "failed to generate git object: unsupported file type at {path:?}"
            ))
        }
    }

    fn decode(raw_content: Vec<u8>) -> Result<Self> {
        let decompressed_content =
            decompress(raw_content).with_context(|| format!("failed to decompress object file"))?;

        let [header_bytes, content]: [&[_]; 2] = decompressed_content
            .splitn(2, |b| b == &b'\0')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| anyhow!("invalid object file: expected it to contain {:?}", "\0"))?;

        let header_str = from_utf8_with_context(header_bytes.to_vec())
            .with_context(|| format!("failed to parse object file header"))?;

        let [object_type_str, content_size_str]: [&str; 2] = header_str
            .splitn(2, ' ')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| {
                anyhow!(
                    "failed to decode git object: expected header to have format {:?} but instead got {:?}",
                    "<type> <size>\0",
                    header_str
                )
            })?;

        let object_type = parse_with_context(object_type_str).with_context(|| {
            format!("failed to decode git object: failed to decode object type")
        })?;

        let content_size = parse_with_context(content_size_str).with_context(|| {
            format!("failed to decode git object: failed to decode content size")
        })?;

        assert_eq!(content.len(), content_size);

        let content = content.to_vec();
        match object_type {
            GitObjectType::Blob => Ok(Self::Blob(Blob::decode_body(content.to_vec())?)),
            GitObjectType::Tree => Ok(Self::Tree(Tree::decode_body(content.to_vec())?)),
            GitObjectType::Commit => Ok(Self::Commit(Commit::decode_body(content.to_vec())?)),
        }
    }
}
