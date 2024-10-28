use anyhow::{anyhow, Context, Result};
use blob::blob::Blob;
#[allow(unused_imports)]
use std::{
    env, fs,
    io::{stdout, Write},
    path::Path,
};

mod blob;

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

            let blob: Blob = raw_content
                .try_into()
                .with_context(|| format!("failed to parse object for {blob_path}"))?;

            stdout
                .write_all(blob.get_blob_first_index())
                .with_context(|| {
                    format!("failed to write object file content to stdout for {blob_path}")
                })?;
        }
        "hash-object" => {
            assert_eq!(args[2], "-w");
            let path = &args[3];

            let content =
                fs::read(path).with_context(|| format!("failed to read file at {path}"))?;
            let blob = Blob(content);
            let sha1 = blob.sha1();

            println!("{}", sha1);

            let object_folder = format!(".git/objects/{}", &sha1[..2]);
            let object_path = format!("{}/{}", &object_folder, &sha1[2..]);

            // println!("Object PATH {}", object_path);

            if !Path::new(&object_folder).exists() {
                fs::create_dir(&object_folder).with_context(|| {
                    format!("failed to create object folder at {object_folder} for {path}")
                })?;
            } else if !fs::metadata(&object_folder)?.is_dir() {
                return Err(anyhow!("object folder is not a directory: {object_folder}"));
            }

            let encoded = blob
                .encode()
                .with_context(|| format!("failed to encode object file for {path}"))?;

            fs::write(&object_path, encoded).with_context(|| {
                format!("failed to write object file at {object_path} for {path}")
            })?;
        }
        command => println!("unknown command: {}", command),
    }

    Ok(())
}
