//! File and byte hashing shared across the kask families.
//!
//! [`sha256_file`] and [`sha256_bytes`] are the one implementation of
//! hex-SHA-256 provenance hashing. They were previously duplicated in
//! three production sites — dme-harness's GGUF writer (8 MiB buffer,
//! `GgufError`), hkask-memory's federated recall (1 MiB stack buffer,
//! `FederatedRecallError`), and hkask-mcp-corpus's calibration tools
//! (64 KiB buffer, `McpToolError`) — with identical semantics and
//! divergent buffers and error wrapping. Unified by the 2026-11-16 kask
//! audit; the callers keep their error types by wrapping the shared
//! `std::io::Error`.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Buffer size for [`sha256_file`]: 1 MiB heap — large enough that hashing
/// a multi-GiB model file stays syscall-cheap, small enough that a small
/// provenance read allocates a modest heap block. The pre-unification
/// copies ranged 64 KiB–8 MiB with identical output.
const FILE_BUFFER_BYTES: usize = 1024 * 1024;

/// Streaming SHA-256 of a file, hex-encoded (64 lowercase chars).
///
/// Reads are streamed through a 1 MiB buffer, so arbitrarily large files
/// hash in constant memory. I/O failures propagate as `std::io::Error` —
/// callers wrap it into their own error types with their own context.
pub fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; FILE_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// In-memory SHA-256, hex-encoded. Formatting mirrors [`sha256_file`].
pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        // FIPS 180-4 test vectors — the empty string and "abc".
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn file_streaming_matches_in_memory() {
        let dir =
            std::env::temp_dir().join(format!("hkask-types-hashing-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test dir");
        let path = dir.join("probe.bin");
        // Larger than one buffer, so the streaming loop actually loops.
        let payload: Vec<u8> = (0..(FILE_BUFFER_BYTES * 2 + 1) as u32)
            .map(|i| (i % 251) as u8)
            .collect();
        std::fs::write(&path, &payload).expect("test payload");
        let streamed = sha256_file(&path).expect("streamed hash");
        let in_memory = sha256_bytes(&payload);
        assert_eq!(streamed, in_memory);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn missing_file_is_an_io_error() {
        assert!(sha256_file(Path::new("/nonexistent/hkask-types-hashing-probe")).is_err());
    }
}
