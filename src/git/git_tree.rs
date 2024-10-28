use crate::git::git_object_trait::GitObject;
use crate::utils::{
    constants::TREE_OBJECT_HEADER_PREFIX,
    helpers::{from_utf8_with_context, into_bytes, parse_bytes_with_context, parse_with_context},
};
use anyhow::{anyhow, Context, Result};
use flate2::write::ZlibDecoder;
use sha::{sha1::Sha1, utils::Digest};
use std::io::Write;
use strum::{AsRefStr, EnumString};

pub struct Tree(pub Vec<TreeEntry>);
pub struct TreeEntry {
    mode: FileMode,
    pub name: String,
    hash: [u8; 20],
}

#[derive(EnumString, AsRefStr)]
enum FileMode {
    #[strum(serialize = "100644")]
    Regular,
    #[strum(serialize = "100755")]
    Executable,
    #[strum(serialize = "120000")]
    Symbolic,
    #[strum(serialize = "40000")]
    Directory,
}

impl TreeEntry {
    fn decode<Iter: IntoIterator<Item = u8>>(iter: Iter) -> Result<Self> {
        let mut iter = iter.into_iter();
        let iter = iter.by_ref();
        let mode: FileMode = parse_bytes_with_context(iter.take_while(|b| b != &b' ').collect())
            .with_context(|| format!("failed to parse tree entry mode"))?;

        let name = from_utf8_with_context(iter.take_while(|b| b != &b'\0').collect())
            .with_context(|| format!("failed to parse tree entry name"))?;

        let hash = iter.take(20).collect::<Vec<_>>().try_into().map_err(|_| {
            anyhow!("failed to parse tree entry sha1: expected it to contain exactly 20 bytes")
        })?;

        Ok(Self { mode, name, hash })
    }

    fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(self.mode.as_ref().as_bytes());
        encoded.push(b' ');
        encoded.extend_from_slice(self.name.as_bytes());
        encoded.push(b'\0');
        encoded.extend_from_slice(&self.hash);
        encoded
    }
}

impl GitObject for Tree {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut buf = format!("{}{}\0", TREE_OBJECT_HEADER_PREFIX, self.0.len()).into_bytes();
        for entry in &self.0 {
            buf.extend_from_slice(&entry.encode());
        }
        Ok(buf)
    }
    fn decode(from: Vec<u8>) -> Result<Self>
    where
        Self: Sized,
    {
        let mut decoder = ZlibDecoder::new(vec![]);
        decoder
            .write(&from)
            .with_context(|| format!("failed to write tree object file content to zlib decoder"))?;

        let decompressed = decoder
            .finish()
            .with_context(|| format!("failed to decompress tree object file content"))?;

        let lossy = String::from_utf8_lossy(&decompressed).to_string();

        let mut iter = decompressed.into_iter();

        let iter_ref = iter.by_ref();

        let header =
            from_utf8_with_context(iter_ref.take_while(|b| b != &b'\0').collect::<Vec<_>>())
                .with_context(|| format!("failed to parse tree object file header"))?;

        let size: usize = parse_with_context(&header[TREE_OBJECT_HEADER_PREFIX.len()..])
            .with_context(|| format!("failed to parse tree object size as integer"))?;
        let mut iter = iter.take(size).peekable();
        let mut entries = vec![];
        while iter.peek().is_some() {
            entries
                .push(TreeEntry::decode(&mut iter).with_context(|| {
                    format!("failed to parse tree object file entry {lossy:?}")
                })?);
        }
        Ok(Tree(entries))
    }
    fn sha1(&self) -> [u8; 20] {
        into_bytes(
            Sha1::default()
                .digest(&self.encode().expect("Tree encoding never fails... for now"))
                .0,
        )
    }
}
