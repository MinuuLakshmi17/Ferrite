use crate::bencode::{decode, encode};
use anyhow::{bail, Context, Result};
use sha1::{Digest, Sha1};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct TorrentMeta {
    pub announce: Vec<String>,
    pub info: Info,
    pub info_hash: [u8; 20],
    pub raw_info: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Info {
    pub name: String,
    pub piece_length: u64,
    pub pieces: Vec<[u8; 20]>,
    pub length: Option<u64>,
    pub files: Vec<FileEntry>,
}
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub length: u64,
    pub path: PathBuf,
}

impl TorrentMeta {
    pub fn from_file(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        Self::from_bytes(&bytes)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let (root, n) = decode(bytes)?;
        if n != bytes.len() {
            bail!("trailing bytes after torrent dictionary")
        };
        let d = root.dict()?;
        let mut announce = Vec::new();
        if let Some(v) = d.get(b"announce".as_slice()) {
            announce.push(v.as_str()?.to_string());
        }
        if let Some(v) = d.get(b"announce-list".as_slice()) {
            for tier in v.list()? {
                for u in tier.list()? {
                    announce.push(u.as_str()?.to_string());
                }
            }
        }
        let info = d
            .get(b"info".as_slice())
            .ok_or_else(|| anyhow::anyhow!("missing key info"))?;
        let mut raw = Vec::new();
        encode(info, &mut raw);
        let digest = Sha1::digest(&raw);
        let mut info_hash = [0u8; 20];
        info_hash.copy_from_slice(&digest);
        let id = info.dict()?;
        let name = String::from_utf8(
            id.get(b"name".as_slice())
                .ok_or_else(|| anyhow::anyhow!("missing key name"))?
                .as_bytes()?
                .to_vec(),
        )?;
        let piece_length = id
            .get(b"piece length".as_slice())
            .ok_or_else(|| anyhow::anyhow!("missing key piece length"))?
            .as_int()? as u64;
        if piece_length == 0 {
            bail!("piece length cannot be zero")
        };
        let pb = id
            .get(b"pieces".as_slice())
            .ok_or_else(|| anyhow::anyhow!("missing key pieces"))?
            .as_bytes()?;
        if pb.len() % 20 != 0 {
            bail!("pieces field must be a multiple of 20 bytes")
        };
        let pieces: Vec<[u8; 20]> = pb.as_chunks::<20>().0.to_vec();
        let mut files = Vec::new();
        let length = if let Some(v) = id.get(b"length".as_slice()) {
            Some(v.as_int()? as u64)
        } else {
            for f in id
                .get(b"files".as_slice())
                .ok_or_else(|| anyhow::anyhow!("missing key files"))?
                .list()?
            {
                let fd = f.dict()?;
                let len = fd
                    .get(b"length".as_slice())
                    .ok_or_else(|| anyhow::anyhow!("missing key length"))?
                    .as_int()? as u64;
                let mut p = PathBuf::new();
                for c in fd
                    .get(b"path".as_slice())
                    .ok_or_else(|| anyhow::anyhow!("missing key path"))?
                    .list()?
                {
                    p.push(String::from_utf8(c.as_bytes()?.to_vec())?);
                }
                files.push(FileEntry { length: len, path: p });
            }
            None
        };
        if length.is_none() && files.is_empty() {
            bail!("info must contain length or files")
        };
        Ok(Self {
            announce,
            info: Info {
                name,
                piece_length,
                pieces,
                length,
                files,
            },
            info_hash,
            raw_info: raw,
        })
    }
    pub fn total_length(&self) -> u64 {
        self.info
            .length
            .unwrap_or_else(|| self.info.files.iter().map(|f| f.length).sum())
    }
    pub fn piece_count(&self) -> usize {
        self.info.pieces.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bencode::{encode, Value};
    use std::collections::BTreeMap;
    #[test]
    fn parses_single() {
        let mut info = BTreeMap::new();
        info.insert(b"length".to_vec(), Value::Int(3));
        info.insert(b"name".to_vec(), Value::Bytes(b"x.txt".to_vec()));
        info.insert(b"piece length".to_vec(), Value::Int(16384));
        info.insert(b"pieces".to_vec(), Value::Bytes(vec![0; 20]));
        let mut root = BTreeMap::new();
        root.insert(b"announce".to_vec(), Value::Bytes(b"http://tracker/test".to_vec()));
        root.insert(b"info".to_vec(), Value::Dict(info));
        let mut b = Vec::new();
        encode(&Value::Dict(root), &mut b);
        let t = TorrentMeta::from_bytes(&b).unwrap();
        assert_eq!(t.total_length(), 3);
        assert_eq!(t.piece_count(), 1);
    }
}
