//! Exercise production startup in child processes: tracing and inventory are process-global.

use std::process::{Command, Stdio};

use hkask_mcp_server::{CredentialRequirement, McpError, run_stdio_server};

const CHILD: &str = "HKASK_BOOTSTRAP_TEST_CHILD";
const REQUIRED: &str = "HKASK_BOOTSTRAP_TEST_REQUIRED";
const OTHER_REQUIRED: &str = "HKASK_BOOTSTRAP_TEST_OTHER_REQUIRED";
const OPTIONAL: &str = "HKASK_BOOTSTRAP_TEST_OPTIONAL";
const SAMPLE_CREDENTIAL: &str = "not-a-secret-bootstrap-fixture";
const SAMPLE_WEBID: &str = "00000000-0000-4000-8000-000000000123";

struct NeverServed;
impl rmcp::ServerHandler for NeverServed {}

#[allow(
    clippy::disallowed_methods,
    reason = "Subprocess isolates tracing and inventory globals; bounded and reaped"
)]
fn run_child(test: &str, injected: &[(&str, &str)]) {
    let temporary = tempfile::tempdir().expect("isolated bootstrap directory");
    let mut child = Command::new(std::env::current_exe().expect("integration-test executable"))
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        // Do not inherit operator credentials, identity, inventory, or keychain access.
        .env_clear()
        .env(CHILD, test)
        .env("HOME", temporary.path())
        .env("XDG_RUNTIME_DIR", temporary.path())
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}/no-session-bus", temporary.path().display()),
        )
        .env(
            hkask_storage::DATABASE_CATALOG_ENV,
            temporary.path().join("inventory.jsonl"),
        )
        .envs(injected.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run isolated production bootstrap");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while child.try_wait().expect("bootstrap child status").is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill timed-out bootstrap child");
            child.wait().expect("reap timed-out bootstrap child");
            panic!("{test} bootstrap child timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let output = child.wait_with_output().expect("bootstrap child output");
    assert!(
        output.status.success(),
        "{test} child failed: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("Capability tier detected"),
        "bootstrap must not run the retired logging-only capability detection: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "child filter must execute the requested test: {:?}",
        output.stdout,
    );
}

/// expect: "Missing required credentials refuse startup before the server factory runs"
#[tokio::test]
async fn missing_required_credentials_refuse_before_factory() {
    let test = "missing_required_credentials_refuse_before_factory";
    if std::env::var(CHILD).as_deref() != Ok(test) {
        run_child(test, &[]);
        return;
    }

    let result = run_stdio_server::<NeverServed, _>(
        "bootstrap-regression",
        env!("CARGO_PKG_VERSION"),
        |_| panic!("factory must not run with missing required credentials"),
        vec![
            CredentialRequirement::required(REQUIRED, "first bootstrap fixture"),
            CredentialRequirement::optional(OPTIONAL, "optional bootstrap fixture"),
            CredentialRequirement::required(OTHER_REQUIRED, "second bootstrap fixture"),
        ],
    )
    .await;

    assert!(
        matches!(&result, Err(McpError::MissingCredentials { missing })
            if missing == &format!("{REQUIRED}, {OTHER_REQUIRED}")),
        "expected all missing required credentials, excluding optional: {result:?}",
    );
}

/// expect: "Absent or invalid process WebID uses the canonical anonymous owner"
#[tokio::test]
async fn default_and_invalid_webid_use_canonical_anonymous_owner() {
    let test = "default_and_invalid_webid_use_canonical_anonymous_owner";
    if std::env::var(CHILD).as_deref() != Ok(test) {
        run_child(test, &[]);
        run_child(test, &[("HKASK_WEBID", "not-a-webid")]);
        return;
    }
    let mut captured = None;
    let result = run_stdio_server::<NeverServed, _>(
        "bootstrap-regression",
        env!("CARGO_PKG_VERSION"),
        |context| {
            captured = Some(context.webid);
            Err(McpError::UnexpectedResponse {
                context: "identity captured".into(),
                detail: "intentionally not serving".into(),
            })
        },
        vec![],
    )
    .await;
    let owner = captured.expect("production bootstrap reaches factory");
    assert_eq!(owner, hkask_types::WebID::anonymous());
    assert_eq!(
        owner.to_string(),
        "ee27e819-68c2-5d33-9289-3443fce69c0a",
        "default identity must remain stable for persisted ownership"
    );
    assert!(matches!(result, Err(McpError::UnexpectedResponse { .. })));
}

/// expect: "Injected credentials and WebID reach the factory without an available keychain"
#[tokio::test]
async fn injected_credentials_and_webid_reach_factory() {
    let test = "injected_credentials_and_webid_reach_factory";
    if std::env::var(CHILD).as_deref() != Ok(test) {
        run_child(
            test,
            &[(REQUIRED, SAMPLE_CREDENTIAL), ("HKASK_WEBID", SAMPLE_WEBID)],
        );
        return;
    }

    let mut captured = None;
    let result = run_stdio_server::<NeverServed, _>(
        "bootstrap-regression",
        env!("CARGO_PKG_VERSION"),
        |context| {
            captured = Some(context);
            // Stop before stdio serving and verify the factory error is preserved.
            Err(McpError::UnexpectedResponse {
                context: "bootstrap factory sentinel".into(),
                detail: "intentionally not serving".into(),
            })
        },
        vec![
            CredentialRequirement::required(REQUIRED, "injected bootstrap fixture"),
            CredentialRequirement::optional(OPTIONAL, "absent optional bootstrap fixture"),
        ],
    )
    .await;

    let context = captured.expect("bootstrap must invoke the factory");
    assert_eq!(context.credentials.len(), 1);
    assert_eq!(
        context.credentials.get(REQUIRED).map(String::as_str),
        Some(SAMPLE_CREDENTIAL)
    );
    assert_eq!(
        context.webid,
        SAMPLE_WEBID
            .parse::<hkask_types::WebID>()
            .expect("fixture WebID"),
    );
    assert!(
        matches!(&result, Err(McpError::UnexpectedResponse { context, detail })
            if context == "bootstrap factory sentinel" && detail == "intentionally not serving"),
        "expected the factory sentinel before stdio: {result:?}",
    );
}
