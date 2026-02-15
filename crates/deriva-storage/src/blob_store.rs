use bytes::Bytes;
use deriva_compute::leaf_store::LeafStore;
use deriva_core::address::CAddr;
use deriva_core::error::{DerivaError, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(Self { root })
    }

    fn blob_path(&self, addr: &CAddr) -> PathBuf {
        let hex = addr.to_hex();
        self.root.join(&hex[..2]).join(&hex[2..4]).join(&hex)
    }

    pub fn put(&self, addr: &CAddr, data: &[u8]) -> Result<u64> {
        let path = self.blob_path(addr);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| DerivaError::Storage(e.to_string()))?;
        }
        fs::write(&path, data).map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(data.len() as u64)
    }

    pub fn get(&self, addr: &CAddr) -> Result<Option<Bytes>> {
        let path = self.blob_path(addr);
        match fs::read(&path) {
            Ok(data) => Ok(Some(Bytes::from(data))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(DerivaError::Storage(e.to_string())),
        }
    }

    pub fn contains(&self, addr: &CAddr) -> bool {
        self.blob_path(addr).exists()
    }

    pub fn remove(&self, addr: &CAddr) -> Result<bool> {
        let path = self.blob_path(addr);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(DerivaError::Storage(e.to_string())),
        }
    }

    pub fn total_size(&self) -> Result<u64> {
        let mut total = 0u64;
        for path in walkdir(&self.root)? {
            total += path.metadata().map_err(|e| DerivaError::Storage(e.to_string()))?.len();
        }
        Ok(total)
    }
}

impl LeafStore for BlobStore {
    fn get_leaf(&self, addr: &CAddr) -> Option<Bytes> {
        self.get(addr).ok().flatten()
    }
}

fn walkdir(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !path.is_dir() {
        return Ok(files);
    }
    for entry in fs::read_dir(path).map_err(|e| DerivaError::Storage(e.to_string()))? {
        let entry = entry.map_err(|e| DerivaError::Storage(e.to_string()))?;
        let p = entry.path();
        if p.is_dir() {
            files.extend(walkdir(&p)?);
        } else {
            files.push(p);
        }
    }
    Ok(files)
}
