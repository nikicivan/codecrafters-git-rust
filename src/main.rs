use anyhow::{anyhow, Context, Result};
use git::{
    any_git_object::AnyGitObject,
    commits::{Commit, CommitActor},
    file_tree::FileTree,
    git_client::GitClient,
    git_object_trait::GitObject,
};
use std::{
    env, fs,
    io::{stdout, Write},
    path::Path,
};
use tokio;

mod git;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    let mut stdout = stdout();

    match args[1].as_str() {
        "init" => {
            fs::create_dir(".git")?;
            fs::create_dir(".git/objects")?;
            fs::create_dir(".git/refs")?;
            fs::write(".git/HEAD", "ref: refs/heads/main\n")?;
            println!("Initialized git directory")
        }
        "cat-file" => {
            assert_eq!(args[2], "-p");
            let blob_sha = &args[3];

            let blob = AnyGitObject::read(blob_sha, ".")
                .with_context(|| format!("failed to read object file content for {blob_sha}"))?
                .try_as_blob()
                .ok_or_else(|| {
                    anyhow!(
                        "failed to parse object file content for {blob_sha}: expected it to be a blob"
                    )
                })?;

            stdout.write_all(blob.content()).with_context(|| {
                format!("failed to write object file content to stdout for {blob_sha}")
            })?;
        }
        "hash-object" => {
            assert_eq!(args[2], "-w");
            let path = &args[3];

            let blob = AnyGitObject::generate(path)
                .with_context(|| format!("failed to generate object file from {path}"))?
                .try_as_blob()
                .ok_or_else(|| {
                    anyhow!("failed to generate object file from {path}: expected it to be a blob")
                })?;

            blob.write(".")
                .with_context(|| format!("failed to write object file for blob from {path}"))?;

            let sha = hex::encode(
                blob.sha1()
                    .with_context(|| "failed to generate blob hash")?,
            );

            println!("{sha}");
        }
        "ls-tree" => {
            assert_eq!(args[2], "--name-only");

            let tree_sha = &args[3];

            let tree = AnyGitObject::read(&tree_sha, ".")
                .with_context(|| format!("failed to parse object file content for {tree_sha}"))?
                .try_as_tree()
                .ok_or_else(|| {
                    anyhow!(
                        "failed to parse object file content for {tree_sha}: expected it to be a tree"
                    )
                })?;

            for entry in tree.entries() {
                println!("{}", entry.name);
            }
        }
        "write-tree" => {
            let file_tree = FileTree::new(
                env::current_dir().with_context(|| "failed to get current directory")?,
            )
            .with_context(|| "failed to create file tree")?;

            #[cfg(debug_assertions)]
            eprintln!("{:#?}", file_tree);

            let tree_object = file_tree.write(".")?;
            let sha = hex::encode(
                tree_object
                    .sha1()
                    .with_context(|| "failed to generate tree hash")?,
            );

            println!("{sha}");
        }
        "commit-tree" => {
            let tree_hash_str = &args[2];
            assert_eq!(args[3], "-p");
            let parent_hash_str = &args[4];
            assert_eq!(args[5], "-m");
            let message = args[6..].join(" ");
            #[cfg(debug_assertions)]
            eprintln!("commit-tree {tree_hash_str} -p {parent_hash_str} -m {message}");

            let tree_hash = hex::decode(tree_hash_str)
                .with_context(|| "failed to decode tree sha")?
                .try_into()
                .map_err(|vec: Vec<_>| {
                    anyhow!(
                        "failed to convert tree sha: expected 20 bytes, got {}",
                        vec.len()
                    )
                })?;

            let parent_hash = hex::decode(parent_hash_str)
                .with_context(|| "failed to decode tree sha")?
                .try_into()
                .map_err(|vec: Vec<_>| {
                    anyhow!(
                        "failed to convert tree sha: expected 20 bytes, got {}",
                        vec.len()
                    )
                })?;

            let mock_actor = CommitActor {
                name: "John Doe".to_string(),
                email: "john.doe@codecrafte.rs".to_string(),
                epoch: 0,
                timezone: "+0000".to_string(),
            };

            let commit = Commit::new(
                tree_hash,
                vec![parent_hash],
                mock_actor,
                None,
                format!("{}\n", message),
            );

            commit
                .write(".")
                .with_context(|| "failed to write commit object")?;
            println!("{}", hex::encode(commit.sha1()?));
        }
        "clone" => {
            let url = &args[2];
            let dir_name = Path::new(&args[3]);
            println!(
                "cloning {url} into {:?}",
                std::path::absolute(dir_name).unwrap()
            );
            assert!(!dir_name.exists(), "directory already exists");
            fs::create_dir(&dir_name).with_context(|| "failed to create directory")?;
            let client = GitClient::new(url).with_context(|| "failed to create GitClient")?;

            client
                .clone(&dir_name)
                .await
                .with_context(|| "failed to negotiate")?;
        }
        command => println!("unknown command: {}", command),
    }

    Ok(())
}
