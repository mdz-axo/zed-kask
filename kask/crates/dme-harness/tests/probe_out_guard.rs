//! Pinning tests for the `dme-harness probe --out` non-empty guard.
//!
//! Relaunching a probe leg onto a non-empty `--out` JSONL file must fail by
//! default so partial receipts from a dead attempt cannot silently mix across
//! server sessions and skew `n`. Fresh files, existing 0-byte files, and
//! explicit `--append` opt-in must succeed.

use dme_harness::probe::{self, ProbeArgs};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

struct TempDir {
    dir: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("dme-probe-out-guard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        Self { dir }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn assert_refusal_message(err_text: &str, path: &Path, expected_bytes: usize) {
    let path_str = path.display().to_string();
    assert!(
        err_text.contains(&path_str),
        "expected error to name file `{path_str}`, got: {err_text}"
    );
    assert!(
        err_text.contains(&format!("{expected_bytes} bytes")),
        "expected error to report size `{expected_bytes} bytes`, got: {err_text}"
    );
    assert!(
        err_text.contains("--append"),
        "expected error to mention `--append`, got: {err_text}"
    );
}

#[test]
fn non_empty_out_is_refused_by_default_and_left_untouched() {
    let tmp = TempDir::new("refuse-helper");
    let out_path = tmp.join("rows.jsonl");
    let prior = "{\"problem_id\":\"p0\",\"label\":\"hit_limit\"}\n";
    std::fs::write(&out_path, prior).expect("seed partial rows file");

    let err = probe::open_rows_file(&out_path, false)
        .expect_err("non-empty --out must be refused when append=false");
    assert_refusal_message(&format!("{err:#}"), &out_path, prior.len());

    let after = std::fs::read_to_string(&out_path).expect("read back rows file");
    assert_eq!(after, prior, "refusal must not modify or truncate the file");
}

#[test]
fn probe_run_refuses_non_empty_out_before_connecting() {
    let tmp = TempDir::new("refuse-run");
    let out_path = tmp.join("rows.jsonl");
    let prior = "{\"problem_id\":\"p0\"}\n";
    std::fs::write(&out_path, prior).expect("seed partial rows file");

    let args = ProbeArgs {
        gguf: tmp.join("nonexistent.gguf"),
        endpoint: "http://127.0.0.1:1".into(),
        config: tmp.join("nonexistent-config.json"),
        problems: tmp.join("nonexistent-problems.jsonl"),
        out_jsonl: out_path.clone(),
        out_summary: None,
        start: 0,
        count: 1,
        cap: 64,
        seed_base: 1,
        budget_secs: 10,
        append: false,
    };

    let err = probe::run(args).expect_err("probe::run must refuse non-empty --out without append");
    assert_refusal_message(&format!("{err:#}"), &out_path, prior.len());
    assert_eq!(
        std::fs::read_to_string(&out_path).expect("read back rows file"),
        prior
    );
}

#[test]
// Synchronous CLI integration test; dme-harness has no async runtime.
#[allow(clippy::disallowed_methods)]
fn probe_cli_refuses_non_empty_out_without_append_flag() {
    let tmp = TempDir::new("refuse-cli");
    let out_path = tmp.join("rows.jsonl");
    let prior = "{\"problem_id\":\"p0\"}\n";
    std::fs::write(&out_path, prior).expect("seed partial rows file");

    let output = Command::new(env!("CARGO_BIN_EXE_dme-harness"))
        .args([
            "probe",
            "--gguf",
            tmp.join("nonexistent.gguf").to_str().unwrap(),
            "--config",
            tmp.join("nonexistent-config.json").to_str().unwrap(),
            "--problems",
            tmp.join("nonexistent-problems.jsonl").to_str().unwrap(),
            "--out",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("run dme-harness probe");

    assert!(
        !output.status.success(),
        "CLI probe must exit non-zero on non-empty --out without --append"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_refusal_message(&stderr, &out_path, prior.len());
    assert_eq!(
        std::fs::read_to_string(&out_path).expect("read back rows file"),
        prior
    );
}

#[test]
fn new_and_empty_out_files_succeed_by_default() {
    let tmp = TempDir::new("new-and-empty");

    // Brand-new file
    let fresh = tmp.join("fresh.jsonl");
    {
        let mut f = probe::open_rows_file(&fresh, false)
            .expect("new --out file should succeed with append=false");
        writeln!(f, "{{\"row\":1}}").expect("write to fresh file");
    }
    assert_eq!(
        std::fs::read_to_string(&fresh).expect("read fresh file"),
        "{\"row\":1}\n"
    );

    // Existing 0-byte file
    let empty = tmp.join("empty.jsonl");
    std::fs::write(&empty, b"").expect("create 0-byte file");
    {
        let mut f = probe::open_rows_file(&empty, false)
            .expect("existing 0-byte --out file should succeed with append=false");
        writeln!(f, "{{\"row\":2}}").expect("write to empty file");
    }
    assert_eq!(
        std::fs::read_to_string(&empty).expect("read empty file"),
        "{\"row\":2}\n"
    );
}

#[test]
fn non_empty_out_succeeds_with_explicit_append_opt_in() {
    let tmp = TempDir::new("append-opt-in");
    let out_path = tmp.join("rows.jsonl");
    let prior = "{\"row\":1}\n";
    std::fs::write(&out_path, prior).expect("seed rows file");

    {
        let mut f = probe::open_rows_file(&out_path, true)
            .expect("non-empty --out should succeed when append=true");
        writeln!(f, "{{\"row\":2}}").expect("append second row");
    }

    assert_eq!(
        std::fs::read_to_string(&out_path).expect("read appended file"),
        "{\"row\":1}\n{\"row\":2}\n"
    );
}
