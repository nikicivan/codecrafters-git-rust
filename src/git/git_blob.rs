use crate::git::git_object_trait::{GitObject, GitObjectType};
use anyhow::Result;

#[derive(Clone)]
#[repr(transparent)]
pub struct BlobContent(pub Vec<u8>);

impl std::fmt::Debug for BlobContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BlobContent")
            .field(&String::from_utf8_lossy(&self.0))
            .finish()
    }
}

impl From<Vec<u8>> for BlobContent {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}
impl Into<Vec<u8>> for BlobContent {
    fn into(self) -> Vec<u8> {
        self.0
    }
}
impl AsRef<Vec<u8>> for BlobContent {
    fn as_ref(&self) -> &Vec<u8> {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct Blob {
    pub content: BlobContent,
}

impl Blob {
    pub fn new<T: Into<BlobContent>>(content: T) -> Self {
        Self {
            content: content.into(),
        }
    }
    pub fn content(&self) -> &Vec<u8> {
        &self.content.as_ref()
    }
}

impl GitObject for Blob {
    fn get_type() -> GitObjectType {
        GitObjectType::Blob
    }

    fn encode_body(&self) -> Result<Vec<u8>> {
        Ok(self.content.clone().into())
    }

    fn decode_body(raw_content: Vec<u8>) -> Result<Self> {
        Ok(Blob {
            content: raw_content.to_vec().into(),
        })
    }
}
