use crate::{
    git::git_object_trait::{GitObject, GitObjectType},
    utils::helpers::{from_utf8_with_context, parse_bytes_with_context},
};
use anyhow::{anyhow, Context, Result};
use bytes::BufMut;
use hex;
use std::io::Write;

#[derive(Debug)]
pub struct CommitActor {
    pub name: String,
    pub email: String,
    pub epoch: u64,
    pub timezone: String,
}

#[derive(Debug)]
pub struct Commit {
    tree_hash: [u8; 20],
    parent_hash: Option<[u8; 20]>,
    author: CommitActor,
    committer: Option<CommitActor>,
    commit_message: String,
}

impl GitObject for Commit {
    fn encode_body(&self) -> Result<Vec<u8>> {
        let mut buf = (vec![]).writer();

        buf.write(format!("tree {}\n", hex::encode(&self.tree_hash)).as_bytes())?;

        if let Some(ref parent_hash) = self.parent_hash {
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
        buf.write(format!("\n{}\n", self.commit_message).as_bytes())?;

        Ok(buf.into_inner())
    }

    fn decode_body(from: Vec<u8>) -> Result<Self> {
        let mut iter = from.into_iter().peekable();

        assert_eq!(&iter.by_ref().take(5).collect::<Vec<_>>(), b"tree ");
        let tree_hash = iter.by_ref().take(20).collect::<Vec<_>>().try_into().map_err(|_| {
          anyhow!("failed to parse commit object file: expected tree hash to contain exactly 20 bytes")
        })?;

        assert_eq!(iter.by_ref().take(1).collect::<Vec<_>>(), b"\n");
        assert_eq!(&iter.by_ref().take(7).collect::<Vec<_>>(), b"parent ");

        let parent_hash = Some(iter.by_ref().take(20).collect::<Vec<_>>().try_into().map_err(|_| {
          anyhow!("failed to parse commit object file: expected parent hash to contain exactly 20 bytes")
        })?);

        assert_eq!(iter.by_ref().take(1).collect::<Vec<_>>(), b"\n");
        assert_eq!(&iter.by_ref().take(7).collect::<Vec<_>>(), b"author ");

        let author_name =
            from_utf8_with_context(iter.by_ref().take_while(|b| b != &b' ').collect())
                .with_context(|| {
                    format!("failed to parse commit object file: failed to parse author name")
                })?;

        let author_email = from_utf8_with_context(
            iter.by_ref()
                .skip_while(|b| b != &b'<')
                .take_while(|b| b != &b'>')
                .collect(),
        )
        .with_context(|| {
            format!("failed to parse commit object file: failed to parse author email")
        })?;

        assert_eq!(iter.by_ref().take(1).collect::<Vec<_>>(), b" ");
        let author_epoch =
            parse_bytes_with_context(iter.by_ref().take_while(|b| b != &b' ').collect())
                .with_context(|| {
                    format!("failed to parse commit object file: failed to parse author epoch")
                })?;

        assert_eq!(iter.by_ref().take(1).collect::<Vec<_>>(), b" ");
        let author_timezone =
            from_utf8_with_context(iter.by_ref().take_while(|b| b != &b'\n').collect())
                .with_context(|| {
                    format!("failed to parse commit object file: failed to parse author timezone")
                })?;

        assert_eq!(&iter.by_ref().take(10).collect::<Vec<_>>(), b"committer ");

        let committer_name =
            from_utf8_with_context(iter.by_ref().take_while(|b| b != &b' ').collect())
                .with_context(|| {
                    format!("failed to parse commit object file: failed to parse committer name")
                })?;

        let committer_email = from_utf8_with_context(
            iter.by_ref()
                .skip_while(|b| b != &b'<')
                .take_while(|b| b != &b'>')
                .collect(),
        )
        .with_context(|| {
            format!("failed to parse commit object file: failed to parse committer email")
        })?;

        assert_eq!(iter.by_ref().take(1).collect::<Vec<_>>(), b" ");

        let committer_epoch =
            parse_bytes_with_context(iter.by_ref().take_while(|b| b != &b' ').collect())
                .with_context(|| {
                    format!("failed to parse commit object file: failed to parse committer epoch")
                })?;

        let committer_timezone =
            from_utf8_with_context(iter.by_ref().take_while(|b| b != &b'\n').collect())
                .with_context(|| {
                    format!(
                        "failed to parse commit object file: failed to parse committer timezone"
                    )
                })?;

        assert_eq!(iter.by_ref().take(1).collect::<Vec<_>>(), b"\n");

        let commit_message = from_utf8_with_context(iter.collect()).with_context(|| {
            format!("failed to parse commit object file: failed to parse commit message")
        })?;

        Ok(Commit {
            tree_hash,
            parent_hash,
            author: CommitActor {
                name: author_name,
                email: author_email,
                epoch: author_epoch,
                timezone: author_timezone,
            },
            committer: Some(CommitActor {
                name: committer_name,
                email: committer_email,
                epoch: committer_epoch,
                timezone: committer_timezone,
            }),
            commit_message,
        })
    }

    fn get_type() -> GitObjectType {
        GitObjectType::Commit
    }
}

impl Commit {
    pub fn new(
        tree_hash: [u8; 20],
        parent_hash: Option<[u8; 20]>,
        author: CommitActor,
        committer: Option<CommitActor>,
        commit_message: String,
    ) -> Self {
        Self {
            tree_hash,
            parent_hash,
            author,
            committer,
            commit_message,
        }
    }
}
