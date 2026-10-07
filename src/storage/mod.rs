use crate::torrent::Info;
use anyhow::{Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub struct Storage {
    root: PathBuf,
    info: Info,
}
impl Storage {
    pub fn new(root: impl AsRef<Path>, info: Info) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            info,
        }
    }
    pub fn path_for(&self, relative: &Path) -> PathBuf {
        self.root.join(relative)
    }
    pub fn write_piece(&self, offset: u64, data: &[u8]) -> Result<()> {
        if self.info.length.is_some() {
            let p = self.root.join(&self.info.name);
            self.write_at(&p, offset, data)
        } else {
            let mut global = offset;
            let mut remaining = data;
            for f in &self.info.files {
                if global >= f.length {
                    global -= f.length;
                    continue;
                }
                let n = ((f.length - global) as usize).min(remaining.len());
                self.write_at(&self.root.join(&self.info.name).join(&f.path), global, &remaining[..n])?;
                remaining = &remaining[n..];
                global = 0;
                if remaining.is_empty() {
                    break;
                }
            }
            Ok(())
        }
    }
    fn write_at(&self, p: &Path, offset: u64, data: &[u8]) -> Result<()> {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        // NOTE: no .truncate(true) here: pieces are written at offsets into
        // preallocated files, so truncating would destroy other pieces.
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(p)?;
        f.seek(SeekFrom::Start(offset))?;
        f.write_all(data)?;
        f.flush()?;
        Ok(())
    }
    pub fn preallocate(&self) -> Result<()> {
        if let Some(n) = self.info.length {
            let p = self.root.join(&self.info.name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let f = File::create(p)?;
            f.set_len(n)?;
        } else {
            for x in &self.info.files {
                let p = self.root.join(&self.info.name).join(&x.path);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                File::create(p)?.set_len(x.length)?;
            }
        }
        Ok(())
    }
}
