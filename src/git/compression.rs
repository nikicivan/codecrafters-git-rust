use anyhow::{Context, Result};
use flate2::write::{ZlibDecoder, ZlibEncoder};
use std::io::Write;

pub fn compress(input: Vec<u8>) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Default::default());

    encoder
        .write_all(&input)
        .with_context(|| format!("failed to write input to zlib encoder"))?;

    encoder
        .finish()
        .with_context(|| format!("failed to finish zlib encoder"))
}

pub fn decompress(input: Vec<u8>) -> Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(Vec::new());
    decoder
        .write_all(&input)
        .with_context(|| format!("failed to write input to zlib decoder"))?;
    decoder
        .finish()
        .with_context(|| format!("failed to finish zlib decoder"))
}
