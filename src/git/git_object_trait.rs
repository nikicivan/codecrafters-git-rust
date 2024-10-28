use crate::utils::helpers::{get_object_file_path, get_object_folder_path};
use anyhow::{anyhow, Context, Result};
use std::{fs, path::Path};

pub trait GitObject {
    fn encode(&self) -> Result<Vec<u8>>;
    fn decode(from: Vec<u8>) -> Result<Self>
    where
        Self: Sized;
    fn sha1(&self) -> [u8; 20];

    fn write(&self) -> Result<()> {
        let encoded = self.encode()?;
        let sha = hex::encode(self.sha1());
        let folder_path = get_object_folder_path(&sha);
        let file_path = get_object_file_path(&sha);

        if !Path::new(&folder_path).exists() {
            fs::create_dir(&folder_path)
                .with_context(|| format!("failed to create object folder at {folder_path}"))?;
        } else if !fs::metadata(&folder_path)?.is_dir() {
            return Err(anyhow!("object folder is not a directory: {folder_path}"));
        }
        fs::write(&file_path, encoded)
            .with_context(|| format!("failed to write object file at {file_path}"))?;
        Ok(())
    }
}
