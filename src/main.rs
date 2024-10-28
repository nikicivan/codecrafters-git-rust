use anyhow::{anyhow, Context, Result};
use flate2::write::ZlibDecoder;
#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::{
    fs,
    io::{stdout, Write},
};

static OBJECT_HEADER_PREFIX: &str = "blob ";

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    let mut stdout = stdout();

    match args[1].as_str() {
        "init" => {
            fs::create_dir(".git").unwrap();
            fs::create_dir(".git/objects").unwrap();
            fs::create_dir(".git/refs").unwrap();
            fs::write(".git/HEAD", "ref: refs/heads/main\n").unwrap();
            println!("Initialized git directory")
        }
        "cat-file" => {
            assert_eq!(args[2], "-p");
            let blog_sha = &args[3];
            let blob_path = format!(".git/objects/{}/{}", &blog_sha[..2], &blog_sha[2..]);

            let raw_content =
                fs::read(&blob_path).with_context(|| format!("failed to read file {blob_path}"))?;

            let mut decoder = ZlibDecoder::new(vec![]);
            decoder.write_all(&raw_content).with_context(|| {
                format!("failed to finish zlib decoder for object file at {blob_path}")
            })?;

            let decompressed_content = decoder.finish().with_context(|| {
                format!("failed to finish zlib decoder for object file at {blob_path}")
            })?;

            let [header, content]: [&[_]; 2] = decompressed_content
                .splitn(2, |b| b == &b'\0')
                .collect::<Vec<_>>()
                .try_into()
                .map_err(|_| {
                    anyhow!(
                        "invalid object file at {blob_path}: expected it to contain {:?}",
                        "\0"
                    )
                })?;

            assert_eq!(
                &header[..OBJECT_HEADER_PREFIX.len()],
                OBJECT_HEADER_PREFIX.as_bytes()
            );

            let content_size: usize =
                String::from_utf8(header[OBJECT_HEADER_PREFIX.len()..].to_vec())
                    .with_context(|| format!("failed to parse object file header as utf8"))?
                    .parse()
                    .with_context(|| format!("failed to parse object file header as integer"))?;

            assert_eq!(content.len(), content_size);

            stdout.write_all(&content).with_context(|| {
                format!("failed to write object file content to stdout for {blob_path}")
            })?;
        }
        command => println!("unknown command: {}", command),
    }

    Ok(())
}
