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

    /// Remove a blob, returning its size in bytes (0 if not found).
    pub fn remove_with_size(&self, addr: &CAddr) -> Result<u64> {
        let path = self.blob_path(addr);
        match fs::metadata(&path) {
            Ok(meta) => {
                let size = meta.len();
                fs::remove_file(&path).map_err(|e| DerivaError::Storage(e.to_string()))?;
                Ok(size)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(DerivaError::Storage(e.to_string())),
        }
    }

    /// Remove multiple blobs. Returns (count_removed, bytes_removed).
    pub fn remove_batch_blobs(&self, addrs: &[CAddr]) -> Result<(u64, u64)> {
        let mut count = 0u64;
        let mut bytes = 0u64;
        for addr in addrs {
            let size = self.remove_with_size(addr)?;
            if size > 0 {
                count += 1;
                bytes += size;
            }
        }
        Ok((count, bytes))
    }

    /// List all blob CAddrs in the store.
    pub fn list_addrs(&self) -> Result<Vec<CAddr>> {
        let mut addrs = Vec::new();
        for path in walkdir(&self.root)? {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(raw) = parse_hex_32(name) {
                    addrs.push(CAddr::from_raw(raw));
                }
            }
        }
        Ok(addrs)
    }

    /// Count total blobs and bytes stored.
    pub fn stats(&self) -> Result<(u64, u64)> {
        let mut count = 0u64;
        let mut bytes = 0u64;
        for path in walkdir(&self.root)? {
            count += 1;
            bytes += path.metadata().map_err(|e| DerivaError::Storage(e.to_string()))?.len();
        }
        Ok((count, bytes))
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

/// Parse a 64-char hex string into [u8; 32].
fn parse_hex_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
