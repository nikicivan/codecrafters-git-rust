use crate::git::{
    git_blob::Blob,
    git_object_trait::GitObject,
    git_tree::{Tree, TreeEntry},
};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct FileTree {
    entries: Vec<FileTreeNode>,
}

#[derive(Debug)]
enum FileTreeNode {
    File(PathBuf),
    Directory(PathBuf, FileTree),
}

impl FileTree {
    pub fn new<T: AsRef<Path>>(path: T) -> Result<Self> {
        let path = path.as_ref();
        let mut entries = vec![];

        let dir_entries = path
            .read_dir()
            .with_context(|| format!("failed to get directory entries at {path:?}"))?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("failed to read directory entry at {path:?}"))?;

        for entry in dir_entries {
            let path = entry.path();

            let file_name = path
                .file_name()
                .with_context(|| format!("failed to get file name from {path:?}"))?;

            if file_name == ".git" {
                continue;
            }

            if path.is_file() {
                entries.push(FileTreeNode::File(path));
            } else if path.is_dir() {
                let subtree = FileTree::new(&path)?;
                entries.push(FileTreeNode::Directory(path, subtree));
            }
        }

        Ok(Self { entries })
    }

    pub fn write(&self) -> Result<Tree> {
        self.parse_tree_object(true)
    }
    pub fn tree_object(&self) -> Result<Tree> {
        self.parse_tree_object(false)
    }

    fn parse_tree_object(&self, should_write: bool) -> Result<Tree> {
        let entries = self
            .entries
            .iter()
            .map(|entry| match entry {
                FileTreeNode::File(path) => {
                    let content = fs::read(path)
                        .with_context(|| format!("failed to read file at {path:?}"))?;
                    let blob = Blob::new(content);
                    if should_write {
                        blob.write().with_context(|| {
                            format!("failed to write object file for blob from {path:?}")
                        })?;
                    }
                    anyhow::Ok(TreeEntry::new(&blob, path).with_context(|| {
                        format!("failed to create tree entry for file at {path:?}")
                    })?)
                }
                FileTreeNode::Directory(path, tree) => {
                    let tree_object = tree.parse_tree_object(should_write)?;
                    anyhow::Ok(TreeEntry::new(&tree_object, path).with_context(|| {
                        format!("failed to create tree entry for directory at {path:?}")
                    })?)
                }
            })
            .collect::<Result<Vec<_>>>()?;

        let tree_object = Tree::new(entries);

        if should_write {
            tree_object
                .write()
                .with_context(|| "failed to write tree object")?;
        }
        Ok(tree_object)
    }
}
