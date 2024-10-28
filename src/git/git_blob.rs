use crate::git::git_object_trait::GitObject;
use crate::utils::{
    constants::BLOB_OBJECT_HEADER_PREFIX,
    helpers::{into_bytes, parse_bytes_with_context},
};
use anyhow::{anyhow, Context, Result};
use bytes::BytesMut;
use flate2::write::{ZlibDecoder, ZlibEncoder};
use sha::{sha1::Sha1, utils::Digest};
use std::io::Write;

pub struct Blob(pub Vec<u8>);

impl Blob {
    fn get_header(&self) -> String {
        format!("{}{}\0", BLOB_OBJECT_HEADER_PREFIX, self.0.len())
    }

    pub fn get_first_index(&self) -> &[u8] {
        &self.0
    }
}

impl GitObject for Blob {
    fn encode(&self) -> Result<Vec<u8>> {
        let content = &self.0;
        let header = self.get_header();
        let mut encoder = ZlibEncoder::new(Vec::new(), Default::default());
        encoder
            .write_all(header.as_bytes())
            .with_context(|| format!("failed to write object file header to zlib encoder"))?;
        encoder
            .write_all(&content)
            .with_context(|| format!("failed to write object file content to zlib encoder"))?;
        encoder
            .finish()
            .with_context(|| format!("failed to finish zlib encoder for object file"))
    }

    fn sha1(&self) -> [u8; 20] {
        let mut hash_input = BytesMut::from(self.get_header().as_bytes());
        hash_input.extend(&self.0);

        into_bytes(Sha1::default().digest(&hash_input).0)
    }

    fn decode(raw_content: Vec<u8>) -> Result<Self>
    where
        Self: Sized,
    {
        let mut decoder = ZlibDecoder::new(vec![]);

        decoder
            .write_all(&raw_content)
            .with_context(|| format!("failed to write object file content to zlib decoder"))?;

        let decompressed_content = decoder
            .finish()
            .with_context(|| format!("failed to finish zlib decoder for object file"))?;

        let [header, content]: [&[_]; 2] = decompressed_content
            .splitn(2, |b| b == &b'\0')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| anyhow!("invalid object file: expected it to contain {:?}", "\0"))?;

        assert_eq!(
            &header[..BLOB_OBJECT_HEADER_PREFIX.len()],
            BLOB_OBJECT_HEADER_PREFIX.as_bytes()
        );

        let content_size: usize =
            parse_bytes_with_context(header[BLOB_OBJECT_HEADER_PREFIX.len()..].to_vec())
                .with_context(|| format!("failed to parse object file header"))?;

        assert_eq!(content.len(), content_size);
        Ok(Blob(content.to_vec()))
    }
}
