use crate::{
    git::{
        any_git_object::Sha,
        git_object_trait::{GitObject, GitObjectType},
    },
    utils::helpers::from_utf8_with_context,
};
use anyhow::{anyhow, Context, Error, Result};
use bytes::BufMut;
use hex;
use std::{io::Write, str::FromStr};

#[derive(Debug, Clone)]
pub struct CommitActor {
    pub name: String,
    pub email: String,
    pub epoch: u64,
    pub timezone: String,
}

impl FromStr for CommitActor {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let vec = s.split(' ').collect::<Vec<_>>();

        let name = vec
            .get(..vec.len() - 3)
            .ok_or_else(|| {
                anyhow!("failed to parse commit object file: failed to find author name")
            })?
            .join(" ");

        let email = vec.get(vec.len() - 3).ok_or_else(|| {
            anyhow!("failed to parse commit object file: failed to find author email")
        })?;

        let epoch = vec.get(vec.len() - 2).ok_or_else(|| {
            anyhow!("failed to parse commit object file: failed to find author epoch")
        })?;

        let timezone = vec.get(vec.len() - 1).ok_or_else(|| {
            anyhow!("failed to parse commit object file: failed to find author timezone")
        })?;

        if email.chars().next() != Some('<') || email.chars().last() != Some('>') {
            return Err(anyhow!(
                "failed to parse commit object file: expected author email to be enclosed in angle brackets"
            ));
        }

        let email = &email[1..email.len() - 1];

        Ok(CommitActor {
            name: name.to_owned(),
            email: email.to_owned(),
            epoch: epoch.parse().with_context(|| {
                format!("failed to parse commit object file: failed to parse author epoch")
            })?,
            timezone: timezone.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub tree_hash: Sha,
    pub parent_hash: Vec<Sha>,
    author: CommitActor,
    committer: Option<CommitActor>,
    commit_message: String,
}

impl GitObject for Commit {
    fn encode_body(&self) -> Result<Vec<u8>> {
        let mut buf = (vec![]).writer();

        buf.write(format!("tree {}\n", hex::encode(&self.tree_hash)).as_bytes())?;

        for parent_hash in &self.parent_hash {
            buf.write(format!("parent {}\n", hex::encode(parent_hash)).as_bytes())?;
        }

        buf.write(
            format!(
                "author {} <{}> {} {}\n",
                self.author.name, self.author.email, self.author.epoch, self.author.timezone,
            )
            .as_bytes(),
        )?;

        let committer = self.committer.as_ref().unwrap_or(&self.author);

        buf.write(
            format!(
                "committer {} <{}> {} {}\n",
                committer.name, committer.email, committer.epoch, committer.timezone,
            )
            .as_bytes(),
        )?;

        buf.write(format!("\n{}", self.commit_message).as_bytes())?;

        Ok(buf.into_inner())
    }

    fn decode_body(from: Vec<u8>) -> Result<Self> {
        let mut iter = from.into_iter().peekable();

        let pairs = std::iter::from_fn({
            let iter = &mut iter;
            move || {
                if iter.peek() == Some(&b'\n') {
                    iter.next();
                    None
                } else {
                    let iter = iter.by_ref();
                    Some((|| -> Result<_> {
                        let key = String::from_utf8(iter.take_while(|b| b != &b' ').collect())
                            .with_context(|| {
                                format!("failed to parse commit object file: failed to parse key")
                            })?;
                        let value = String::from_utf8(iter.take_while(|b| b != &b'\n').collect())
                            .with_context(|| {
                            format!("failed to parse commit object file: failed to parse value")
                        })?;
                        Ok((key, value))
                    })())
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| {
            format!("failed to parse commit object file: failed to parse key-value pairs")
        })?;

        let tree_hash = Sha(pairs
            .iter()
            .find(|(k, _)| k == "tree")
            .map(|(_, v)| -> Result<[u8; 20]> {
                hex::decode(v).with_context(|| {
                    format!("failed to parse commit object file: failed to parse tree hash: {v:#?}")
                })?.try_into().map_err(|_| {
                    anyhow!("failed to parse commit object file: expected tree hash to contain exactly 20 bytes")
                })
            })
            .ok_or_else(|| anyhow!("failed to parse commit object file: failed to find tree hash"))??);

        let parent_hashes = pairs
            .iter()
            .filter(|(k, _)| k == "parent")
            .map(|(_, v)| -> Result<[u8; 20]> {
                hex::decode(v).with_context(|| {
                    format!("failed to parse commit object file: failed to parse parent hash: {v:#?}")
                })?.try_into().map_err(|_| {
                    anyhow!("failed to parse commit object file: expected parent hash to contain exactly 20 bytes")
                })
            })
            .map(|arr| arr.map(Sha))
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| {
                format!("failed to parse commit object file: failed to parse parent hashes")
            })?;

        let author = pairs
            .iter()
            .find(|(k, _)| k == "author")
            .map(|(_k, v)| CommitActor::from_str(v))
            .ok_or_else(|| {
                anyhow!("failed to parse commit object file: failed to find author")
            })??;

        let committer = pairs
            .iter()
            .find(|(k, _)| k == "committer")
            .map(|(_k, v)| CommitActor::from_str(v))
            .transpose()
            .with_context(|| {
                anyhow!("failed to parse commit object file: failed to find committer")
            })?;

        let commit_message = from_utf8_with_context(iter.collect()).with_context(|| {
            format!("failed to parse commit object file: failed to parse commit message")
        })?;

        let commit = Commit {
            tree_hash,
            parent_hash: parent_hashes,
            author,
            committer,
            commit_message,
        };

        Ok(commit)
    }

    fn get_type() -> GitObjectType {
        GitObjectType::Commit
    }
}

impl Commit {
    pub fn new(
        tree_hash: [u8; 20],
        parent_hashes: Vec<[u8; 20]>,
        author: CommitActor,
        committer: Option<CommitActor>,
        commit_message: String,
    ) -> Self {
        Self {
            tree_hash: tree_hash.into(),
            parent_hash: parent_hashes.into_iter().map(Into::into).collect(),
            author,
            committer,
            commit_message,
        }
    }
}
