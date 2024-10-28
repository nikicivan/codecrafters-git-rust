use anyhow::{Context, Result};
use flate2::read::ZlibDecoder as ZlibReadDecoder;
use flate2::write::{ZlibDecoder, ZlibEncoder};
use std::io::{Read, Write};

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

struct IterRead<I: Iterator<Item = u8>> {
    iter: I,
}

impl<I: Iterator<Item = u8>> Read for &mut IterRead<I> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut bytes_read = 0;

        for i in buf.iter_mut() {
            if let Some(byte) = self.iter.next() {
                *i = byte;
                bytes_read += 1;
            } else {
                return Ok(bytes_read);
            }
        }

        Ok(buf.len())
    }
}

pub fn decompress_slice(content: &[u8]) -> Result<(Vec<u8>, u64)> {
    let mut decoder = ZlibReadDecoder::new(content);

    let mut buff = vec![];
    let buff_size = decoder
        .read_to_end(&mut buff)
        .with_context(|| format!("decompress_up_to_size: failed to finish zlib decoder"))?;

    if false {
        println!(
            "decompress_up_to_size: got {buff_size} bytes from {} bytes",
            decoder.total_in()
        );
    }

    Ok((buff, decoder.total_in()))
}
