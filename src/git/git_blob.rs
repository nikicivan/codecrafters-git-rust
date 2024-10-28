use crate::git::git_object_trait::{GitObject, GitObjectType};
use anyhow::Result;

#[derive(Debug)]
pub struct Blob {
    pub content: Vec<u8>,
}

impl Blob {
    pub fn new(content: Vec<u8>) -> Self {
        Self { content }
    }
    pub fn content(&self) -> &Vec<u8> {
        &self.content
    }
}

impl GitObject for Blob {
    fn get_type() -> GitObjectType {
        GitObjectType::Blob
    }

    fn encode_body(&self) -> Result<Vec<u8>> {
        Ok(self.content.clone())
    }

    fn decode_body(raw_content: Vec<u8>) -> Result<Self> {
        Ok(Blob {
            content: raw_content.to_vec(),
        })
    }
}
