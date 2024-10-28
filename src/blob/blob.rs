use anyhow::{anyhow, Context, Error, Result};
use bytes::BytesMut;
use flate2::write::{ZlibDecoder, ZlibEncoder};
use sha::{
    sha1::Sha1,
    utils::{Digest, DigestExt},
};
use std::io::Write;

pub static OBJECT_HEADER_PREFIX: &str = "blob ";

pub struct Blob(pub Vec<u8>);

impl Blob {
    pub fn get_blob_first_index(&self) -> &[u8] {
        &self.0
    }

    fn get_header(&self) -> String {
        format!("{}{}\0", OBJECT_HEADER_PREFIX, self.0.len())
    }

    pub fn sha1(&self) -> String {
        let mut hash_input = BytesMut::from(self.get_header().as_bytes());
        hash_input.extend(&self.0);
        Sha1::default().digest(&hash_input).to_hex()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
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
}

impl TryFrom<Vec<u8>> for Blob {
    type Error = Error;

    fn try_from(raw_content: Vec<u8>) -> std::result::Result<Self, Self::Error> {
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
            &header[..OBJECT_HEADER_PREFIX.len()],
            OBJECT_HEADER_PREFIX.as_bytes()
        );

        let content_size: usize = String::from_utf8(header[OBJECT_HEADER_PREFIX.len()..].to_vec())
            .with_context(|| format!("failed to parse object file header as utf8"))?
            .parse()
            .with_context(|| format!("failed to parse object file header as integer"))?;

        assert_eq!(content.len(), content_size);

        Ok(Blob(content.to_vec()))
    }
}
