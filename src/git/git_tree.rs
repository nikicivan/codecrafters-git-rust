use crate::{
    git::{any_git_object::Sha, git_object_trait::GitObject, git_object_trait::GitObjectType},
    utils::helpers::{from_utf8_with_context, parse_bytes_with_context},
};
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use strum::{AsRefStr, EnumString};

#[derive(Debug, Clone)]
pub struct Tree(pub Vec<TreeEntry>);
#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub mode: FileMode,
    pub name: String,
    pub hash: Sha,
}

#[derive(Debug, EnumString, AsRefStr, Clone)]
pub enum FileMode {
    #[strum(serialize = "100644")]
    Regular,
    #[strum(serialize = "100755")]
    Executable,
    #[strum(serialize = "120000")]
    Symbolic,
    #[strum(serialize = "40000")]
    Directory,
}

impl From<fs::Metadata> for FileMode {
    fn from(metadata: fs::Metadata) -> Self {
        if metadata.is_dir() {
            Self::Directory
        } else if metadata.permissions().mode() & 0o111 != 0 {
            Self::Executable
        } else if metadata.is_symlink() {
            Self::Symbolic
        } else {
            Self::Regular
        }
    }
}

impl Tree {
    pub fn new(mut entries: Vec<TreeEntry>) -> Self {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Self(entries)
    }
    pub fn entries(&self) -> &Vec<TreeEntry> {
        &self.0
    }
}

impl TreeEntry {
    pub fn new<Obj: GitObject, P: AsRef<Path>>(object: &Obj, path: P) -> Result<Self> {
        let path = path.as_ref();
        let metadata = path.metadata().with_context(|| {
            format!("failed to create tree entry: failed to get metadata for file at {path:?}")
        })?;

        Ok(TreeEntry {
            hash: object.sha1()
                .with_context(|| format!("failed to generate git tree entry: hash generation failed for blob at {path:?}"))?,
            mode: metadata.into(),
            name: path
                .file_name()
                .with_context(|| format!("failed to get file name from {path:?}"))?
                .to_str()
                .ok_or_else(|| {
                    anyhow!("failed to convert file name to string from {path:?}")
                })?
                .to_owned(),
        })
    }

    fn decode<Iter: IntoIterator<Item = u8>>(iter: Iter) -> Result<Self> {
        let mut iter = iter.into_iter();
        let iter = iter.by_ref();

        let mode: FileMode = parse_bytes_with_context(iter.take_while(|b| b != &b' ').collect())
            .with_context(|| format!("failed to parse tree entry mode"))?;

        let name = from_utf8_with_context(iter.take_while(|b| b != &b'\0').collect())
            .with_context(|| format!("failed to parse tree entry name"))?;

        let hash = Sha(iter.take(20).collect::<Vec<_>>().try_into().map_err(|_| {
            anyhow!("failed to parse tree entry sha1: expected it to contain exactly 20 bytes")
        })?);

        Ok(Self { mode, name, hash })
    }

    fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(self.mode.as_ref().as_bytes());
        encoded.push(b' ');
        encoded.extend_from_slice(self.name.as_bytes());
        encoded.push(b'\0');
        encoded.extend_from_slice(&self.hash.as_ref());
        encoded
    }
}

impl GitObject for Tree {
    fn get_type() -> GitObjectType {
        GitObjectType::Tree
    }

    fn encode_body(&self) -> Result<Vec<u8>> {
        let mut body_buf = vec![];
        for entry in &self.0 {
            body_buf.extend_from_slice(&entry.encode());
        }

        Ok(body_buf)
    }

    fn decode_body(from: Vec<u8>) -> Result<Self> {
        let mut iter = from.into_iter().peekable();
        let mut entries = vec![];
        while iter.peek().is_some() {
            entries.push(
                TreeEntry::decode(&mut iter)
                    .with_context(|| format!("failed to parse tree object file entry"))?,
            );
        }
        Ok(Tree::new(entries))
    }
}
