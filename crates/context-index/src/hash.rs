use std::path::Path;

use context_core::ContextError;

/// Max file size for hashing to avoid reading gigantic binaries.
/// 10 MB — see docs for rationale: balances incremental freshness vs memory.
/// Larger files are still discoverable but hash is skipped (returns None).
const MAX_HASH_SIZE: u64 = 10 * 1024 * 1024;

/// Hash file bytes with blake3, streaming.
/// Returns hex digest. Skips files larger than `MAX_HASH_SIZE` (returns error variant).
pub fn hash_file(path: &Path) -> Result<String, ContextError> {
    let meta = std::fs::metadata(path)
        .map_err(|e| ContextError::Internal(format!("cannot stat {}: {}", path.display(), e)))?;
    if meta.len() > MAX_HASH_SIZE {
        return Err(ContextError::Internal(format!(
            "file too large for hashing ({} bytes > {}): {}",
            meta.len(),
            MAX_HASH_SIZE,
            path.display()
        )));
    }
    // Stream read
    let mut hasher = blake3::Hasher::new();
    let mut file = std::fs::File::open(path)
        .map_err(|e| ContextError::Internal(format!("cannot open {}: {}", path.display(), e)))?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        use std::io::Read;
        let n = file.read(&mut buf).map_err(|e| {
            ContextError::Internal(format!("read failed {}: {}", path.display(), e))
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn deterministic() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("a.txt");
        std::fs::write(&p, b"hello").unwrap();
        let h1 = hash_file(&p).unwrap();
        let h2 = hash_file(&p).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn changes_after_edit() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("a.txt");
        std::fs::write(&p, b"hello").unwrap();
        let h1 = hash_file(&p).unwrap();
        std::fs::write(&p, b"hello world").unwrap();
        let h2 = hash_file(&p).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn large_file_skipped() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("big.bin");
        // Create a file larger than MAX_HASH_SIZE via sparse
        let file = std::fs::File::create(&p).unwrap();
        file.set_len(MAX_HASH_SIZE + 1).unwrap();
        let r = hash_file(&p);
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("too large"));
    }
}
