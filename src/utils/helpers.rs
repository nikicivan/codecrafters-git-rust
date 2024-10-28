use anyhow::{anyhow, Context, Result};
use std::str::FromStr;

pub fn get_object_folder_path(sha1: &str) -> String {
    format!(".git/objects/{}", &sha1[..2])
}

pub fn get_object_file_path(sha1: &str) -> String {
    format!("{}/{}", get_object_folder_path(sha1), &sha1[2..])
}

pub fn into_bytes(input: [u32; 5]) -> [u8; 20] {
    input
        .into_iter()
        .flat_map(|val| val.to_be_bytes())
        .collect::<Vec<_>>()
        .try_into()
        .expect("Sha1 digest is always 20 bytes")
}

pub fn from_utf8_with_context(input: Vec<u8>) -> Result<String> {
    String::from_utf8(input).map_err(|err| {
        anyhow!(err.utf8_error()).context(format!(
            "failed to parse as utf8, got {:?}",
            String::from_utf8_lossy(err.as_bytes())
        ))
    })
}

pub fn parse_with_context<Output: FromStr>(input: &str) -> Result<Output>
where
    Output::Err: std::error::Error + Send + Sync + 'static,
{
    input
        .parse()
        .with_context(|| format!("failed to parse {:?}", input))
}

pub fn parse_bytes_with_context<Output: FromStr>(input: Vec<u8>) -> Result<Output>
where
    Output::Err: std::error::Error + Send + Sync + 'static,
{
    from_utf8_with_context(input).and_then(|str| parse_with_context(&str))
}
