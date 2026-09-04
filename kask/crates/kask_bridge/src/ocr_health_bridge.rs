//! OCR health source bridge.
//!
//! Implements `hkask_regulation::OcrHealthSource` over the corpus MCP
//! server's cross-process health file (`{kask_data_dir}/mcp/corpus/
//! ocr-health.json`, the shared `hkask_types::ocr_health` contract). The
//! corpus subprocess's `OcrHealthRecorder` appends silent-failure events
//! atomically; this source reads the snapshot each regulation tick and
//! counts the entries inside the recent window.
//!
//! ## The blind-feedback-loop gap this closes
//!
//! Without this source, the cybernetics loop reports `signal_count=0`
//! during an OCR silent-failure storm (a dead-but-responsive OCR endpoint
//! returning HTTP 200 with empty content on every Complex page). The
//! `reg.pipeline.ocr.silent_failure` warns live in the corpus subprocess's
//! tracing — the loop's existing sensors read ledger/DB state in the zed
//! main process. This is the same trap as `InferenceHealthSource` /
//! `ContextServerHealthSource`, but the events cross a process boundary,
//! hence the file channel instead of an in-process snapshot.

/// Count silent failures from this many seconds before "now". Matches the
/// scale of a corpus-conversion run — a storm persists for the minutes the
/// pipeline runs, while a single transient empty page ages out quickly.
const RECENT_WINDOW_SECS: i64 = 600;

/// A `OcrHealthSource` over the corpus server's health file.
///
/// The composition root creates one `Arc<BridgeOcrHealthSource>` and passes
/// it to `CyberneticsLoop::with_ocr_health_source`. The file is re-read on
/// every sense call — the write side is atomic (tmp+rename), so a read never
/// observes a torn file.
pub struct BridgeOcrHealthSource {
    path: std::path::PathBuf,
}

impl BridgeOcrHealthSource {
    /// Create the source at the canonical health-file path
    /// (`hkask_types::ocr_health::ocr_health_path`).
    pub fn new() -> Self {
        Self {
            path: hkask_types::ocr_health::ocr_health_path(),
        }
    }

    /// Create the source at an explicit path — the test seam.
    #[cfg(test)]
    fn at(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl Default for BridgeOcrHealthSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl hkask_regulation::OcrHealthSource for BridgeOcrHealthSource {
    async fn recent_silent_failures(&self) -> Result<u64, hkask_regulation::OcrHealthError> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            // A missing file is the legitimate "no OCR has run yet" state,
            // not a broken sensor — Ok(0), matching `latest_run_metrics`'
            // NotFound classification. Only a present-but-unreadable file
            // is an error (warned by the sensor, never collapsed to 0).
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => {
                return Err(hkask_regulation::OcrHealthError::Unreadable {
                    path: self.path.clone(),
                    source: error,
                });
            }
        };
        let snapshot: hkask_types::ocr_health::OcrHealthSnapshot = serde_json::from_str(&contents)
            .map_err(|error| hkask_regulation::OcrHealthError::Unparseable {
                path: self.path.clone(),
                source: error,
            })?;
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Ok(snapshot.silent_failures_within(RECENT_WINDOW_SECS, now_unix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_regulation::OcrHealthSource;
    use hkask_types::ocr_health::OcrHealthSnapshot;

    fn temp_health_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ocr-health-test-{name}-{}", std::process::id()))
    }

    fn write_snapshot(path: &std::path::Path, snapshot: &OcrHealthSnapshot) {
        std::fs::write(path, serde_json::to_string(snapshot).expect("serialize"))
            .expect("write test health file");
    }

    #[tokio::test]
    async fn missing_file_is_zero_not_an_error() {
        let source = BridgeOcrHealthSource::at(temp_health_path("missing"));
        assert_eq!(
            source
                .recent_silent_failures()
                .await
                .expect("missing file is Ok(0)"),
            0
        );
    }

    #[tokio::test]
    async fn storm_file_counts_recent_entries() {
        let path = temp_health_path("storm");
        let mut snapshot = OcrHealthSnapshot::default();
        // Two recent (inside the 600s window) + one stale entry.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        snapshot.record_silent_failure(now - 10);
        snapshot.record_silent_failure(now - 20);
        snapshot.record_silent_failure(now - 5_000);
        write_snapshot(&path, &snapshot);

        let source = BridgeOcrHealthSource::at(path);
        assert_eq!(
            source
                .recent_silent_failures()
                .await
                .expect("storm file must parse"),
            2
        );
    }

    #[tokio::test]
    async fn corrupt_file_is_an_error_not_zero() {
        let path = temp_health_path("corrupt");
        std::fs::write(&path, "not json").expect("write corrupt file");
        let path_display = path.display().to_string();
        let source = BridgeOcrHealthSource::at(path);
        let error = source
            .recent_silent_failures()
            .await
            .expect_err("a corrupt health file must be Err, not Ok(0)");
        assert!(
            error.to_string().contains(&path_display),
            "error must name the file: {error}"
        );
    }
}
