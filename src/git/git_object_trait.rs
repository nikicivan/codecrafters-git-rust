use crate::{
    git::{
        any_git_object::Sha,
        compression::{compress, decompress},
    },
    utils::helpers::{get_object_file_path, get_object_folder_path},
};
use anyhow::{anyhow, Context, Result};
use sha::{sha1::Sha1, utils::Digest};
use std::{fs, path::Path};
use strum::{AsRefStr, EnumString};

#[derive(EnumString, AsRefStr, Debug)]
pub enum GitObjectType {
    #[strum(serialize = "blob")]
    Blob,
    #[strum(serialize = "tree")]
    Tree,
    #[strum(serialize = "commit")]
    Commit,
}

pub trait GitObject: Sized {
    fn encode_body(&self) -> Result<Vec<u8>>;
    fn decode_body(from: Vec<u8>) -> Result<Self>;
    fn get_type() -> GitObjectType;
    fn get_header(&self) -> Result<String> {
        Ok(format!(
            "{} {}\0",
            Self::get_type().as_ref(),
            self.encode_body()?.len()
        ))
    }

    fn sha1(&self) -> Result<Sha> {
        Ok(Sha((Sha1::default()
            .digest(
                &self
                    .encode_uncompressed()
                    .with_context(|| format!("failed to generate object hash: encoding failed"))?,
            )
            .0)
            .into_iter()
            .flat_map(|v| v.to_be_bytes())
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| {
                anyhow!("unreachable: [u32; 5] couldn't be converted to [u8; 20]")
            })?))
    }

    fn encode_uncompressed(&self) -> Result<Vec<u8>> {
        let mut buf = self
            .get_header()
            .with_context(|| format!("failed to encode git object: get_header failed"))?
            .into_bytes();

        buf.extend(
            self.encode_body()
                .with_context(|| format!("failed to encode git object: encode_body failed"))?,
        );
        Ok(buf)
    }

    fn encode(&self) -> Result<Vec<u8>> {
        compress(self.encode_uncompressed()?)
            .with_context(|| format!("failed to encode git object: content compression failed"))
    }

    fn write<P: AsRef<Path> + ?Sized>(&self, path: &P) -> Result<()> {
        let encoded = self.encode()?;
        let sha = hex::encode(
            self.sha1()
                .with_context(|| "failed to write object: hash failed")?,
        );

        let folder_path = get_object_folder_path(&sha, path);
        let file_path = get_object_file_path(&sha, path);

        if !Path::new(&folder_path).exists() {
            fs::create_dir_all(&folder_path)
                .with_context(|| format!("failed to create object folder at {folder_path:?}"))?;
        } else if !fs::metadata(&folder_path)?.is_dir() {
            return Err(anyhow!("object folder is not a directory: {folder_path:?}"));
        }

        if false {
            #[cfg(debug_assertions)]
            eprintln!(
                "Writing object file at {file_path:?}: {:?}",
                String::from_utf8_lossy(&decompress(encoded.clone())?)
            );
        }

        fs::write(&file_path, encoded)
            .with_context(|| format!("failed to write object file at {file_path:?}"))?;
        Ok(())
    }
}
