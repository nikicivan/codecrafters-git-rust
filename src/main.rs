use anyhow::{Context, Result};
use git::{git_blob::Blob, git_object_trait::GitObject, git_tree::Tree};
#[allow(unused_imports)]
use std::{
    env, fs,
    io::{stdout, Write},
    path::Path,
};
use utils::helpers::get_object_file_path;

mod git;
mod utils;

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
            let blob_sha = &args[3];

            let blob_path = get_object_file_path(&blob_sha);

            let raw_content =
                fs::read(&blob_path).with_context(|| format!("failed to read file {blob_path}"))?;

            let blob: Blob = Blob::decode(raw_content)
                .with_context(|| format!("failed to parse object file content for {blob_path}"))?;

            stdout
                .write_all(Blob::get_first_index(&blob))
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

            let sha = hex::encode(blob.sha1());

            blob.write()
                .with_context(|| format!("failed to write object file for blob from {path}"))?;

            println!("{sha}");
        }
        "ls-tree" => {
            assert_eq!(args[2], "--name-only");

            let tree_sha = &args[3];
            let tree_path = get_object_file_path(&tree_sha);
            let raw_content = fs::read(&tree_path)
                .with_context(|| format!("failed to read object file at {tree_path}"))?;

            let tree: Tree = Tree::decode(raw_content)
                .with_context(|| format!("failed to parse object file content for {tree_path}"))?;

            for entry in tree.0 {
                println!("{}", entry.name);
            }
        }
        command => println!("unknown command: {}", command),
    }

    Ok(())
}
