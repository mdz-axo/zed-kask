//! Loopback HTTP qualification, never a provider call or scientific calibration.
use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Instant;

struct Reply {
    status: u16,
    bytes: Vec<u8>,
    pause: bool,
    partial: bool,
}

impl Reply {
    fn json(value: Value) -> Self {
        Self {
            status: 200,
            bytes: serde_json::to_vec(&value).expect("fixture JSON"),
            pause: false,
            partial: false,
        }
    }
}

fn count_reply() -> Reply {
    Reply::json(json!({"object":"response.input_tokens","input_tokens":10}))
}

fn generation_reply() -> Reply {
    Reply::json(json!({
        "id":"resp_mock_native", "object":"response", "model":"gpt-5.4-mini-2026-03-17",
        "status":"completed", "error":null, "incomplete_details":null,
        "output":[{"type":"message","role":"assistant","status":"completed","content":[{
            "type":"output_text", "text":"{\"verdict\":\"complete\",\"evidence_refs\":[],\"reason\":\"local mock judgment\"}"
        }]}],
        "usage":{"input_tokens":10,"output_tokens":10,"output_tokens_details":{"reasoning_tokens":0}}
    }))
}

fn policy() -> Protocol {
    let mut protocol = smoke_protocol();
    protocol.limits.spending_ceiling_microusd = 10_000_000;
    protocol.limits.count_request_max_fee_microusd = Some(7);
    protocol
}

fn read_request(stream: &mut TcpStream) -> (String, Value) {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("bounded server read");
    let mut bytes = Vec::new();
    let end = loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).expect("request header byte");
        bytes.push(byte[0]);
        assert!(bytes.len() < 8192, "bounded fixture headers");
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
    };
    let headers = String::from_utf8(bytes.clone()).expect("HTTP headers");
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().expect("length"))
        })
        .expect("content-length");
    assert!(length <= MAX_REQUEST_BYTES);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).expect("exact request body");
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("authorization: bearer local-mock-not-a-secret")
    );
    assert!(!headers.to_ascii_lowercase().contains("transfer-encoding:"));
    let path = headers
        .lines()
        .next()
        .expect("request line")
        .split_whitespace()
        .nth(1)
        .expect("path")
        .to_string();
    assert!(headers.starts_with("POST "));
    assert_eq!(end, bytes.len());
    (path, serde_json::from_slice(&body).expect("JSON request"))
}

/// Assert durable admission and exact payload binding at the receiving socket,
/// before returning any response, not by inspecting a reconstructed replay.
fn server(
    replies: Vec<Reply>,
    output: PathBuf,
) -> (String, thread::JoinHandle<Vec<(String, Value)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback only");
    let base = format!(
        "http://{}/v1",
        listener.local_addr().expect("local address")
    );
    listener.set_nonblocking(true).expect("bounded accept");
    let handle = thread::spawn(move || {
        let mut captures = Vec::new();
        for reply in replies {
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "expected dispatcher request never arrived"
                        );
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("mock accept: {error}"),
                }
            };
            let (path, body) = read_request(&mut stream);
            let journal =
                fs::read_to_string(output.join("budget.jsonl")).expect("journal precedes HTTP");
            let event: Value = serde_json::from_str(journal.lines().last().expect("durable event"))
                .expect("journal JSON");
            let id = event["event"]["id"].as_str().expect("attempt ID");
            let count = path.ends_with("/input_tokens");
            assert_eq!(
                event["event"]["event"],
                if count { "count" } else { "reserve" }
            );
            let filename = if count {
                format!("{id}.count.request.json")
            } else {
                format!("{id}.request.json")
            };
            let stored: Value = read_json(&output.join(filename)).expect("durable bound request");
            assert_eq!(
                stored["body"], body,
                "wire payload must equal frozen request"
            );
            if count {
                assert_eq!(path, "/v1/responses/input_tokens");
                assert_eq!(event["event"]["max_fee_microusd"], 7);
                assert_eq!(
                    stored["generation_body_sha256"]
                        .as_str()
                        .expect("hash")
                        .len(),
                    64
                );
            } else {
                assert_eq!(path, "/v1/responses");
                let frozen: Value = read_json(&output.join(format!("{id}.count.request.json")))
                    .expect("prior count request");
                assert_eq!(
                    frozen["body"],
                    native::count_body(&body).expect("same projection")
                );
                assert_eq!(
                    frozen["generation_body_sha256"],
                    digest(&serde_json::to_vec(&body).expect("canonical bytes"))
                );
                assert_eq!(body["max_output_tokens"], 1024);
                assert_eq!(body["tools"], json!([]));
                assert!(audit(&output).expect("durable admission audit")["outstanding_generation_microusd"].as_u64().expect("liability") > 0);
            }
            captures.push((path, body));
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .expect("write timeout");
            let length = reply.bytes.len() + usize::from(reply.partial);
            let header = format!(
                "HTTP/1.1 {} mock\r\nContent-Length: {length}\r\nContent-Type: application/json\r\nX-Request-Id: req_mock_native\r\nRetry-After: 1\r\nAuthorization: must-not-be-retained\r\nLocation: http://127.0.0.1:1/no-follow\r\nConnection: close\r\n\r\n",
                reply.status
            );
            if reply.pause && !reply.partial {
                thread::sleep(Duration::from_millis(150));
            }
            let written = stream
                .write_all(header.as_bytes())
                .and_then(|()| stream.write_all(&reply.bytes));
            if let Err(error) = written {
                assert!(reply.pause, "unexpected mock write failure: {error}");
            }
            if reply.pause && reply.partial {
                thread::sleep(Duration::from_millis(150));
            }
        }
        let deadline = Instant::now() + Duration::from_millis(60);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok(_) => panic!("unexpected retry, redirect, fallback or post-stop HTTP"),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("final accept: {error}"),
            }
        }
        captures
    });
    (base, handle)
}

fn run_mock(
    protocol: &Protocol,
    cases: &[Case],
    replies: Vec<Reply>,
    output: &Path,
    timeout: Duration,
) -> (Value, Vec<(String, Value)>) {
    let (base, handle) = server(replies, output.to_path_buf());
    let dispatcher = Dispatcher::new(
        protocol,
        Some("local-mock-not-a-secret"),
        Some(base),
        timeout,
    )
    .expect("mock dispatcher");
    let result = run_source(protocol, cases, Source::Http(Box::new(dispatcher)), output)
        .expect("controller report");
    (
        result["report"].clone(),
        handle.join().expect("mock server assertions"),
    )
}

#[test]
fn complete_http_workflow_runs_shared_controller_and_keeps_mock_label() {
    let dir = tempfile::tempdir().expect("temp");
    let output = dir.path().join("run");
    let protocol = policy();
    let cases = evidence::instrument_cases();
    let attempts = cases.len() * 7 + 2;
    let mut replies = Vec::new();
    for _ in 0..attempts {
        replies.extend([count_reply(), generation_reply()]);
    }
    let (report, captures) = run_mock(&protocol, &cases, replies, &output, Duration::from_secs(3));
    assert_eq!(captures.len(), attempts * 2);
    assert_eq!(report["mode"], "local_mock_http");
    assert_eq!(report["budget"]["attempts"], attempts);
    assert_eq!(report["budget"]["count_attempts"], attempts);
    assert_eq!(report["budget"]["outstanding_count_microusd"], attempts * 7);
    assert_eq!(report["budget"]["outstanding_generation_microusd"], 0);
    assert!(report["budget"]["stopped"].is_null());
    assert!(
        report["branches"]
            .as_array()
            .expect("branches")
            .iter()
            .all(|b| b["judgment"].is_object())
    );
    let receipt: Value = read_json(&output.join("cap-0.receipt.json")).expect("receipt");
    assert_eq!(receipt["response_id"], "resp_mock_native");
    assert_eq!(
        receipt["response_headers"]["x-request-id"],
        "req_mock_native"
    );
    assert_eq!(receipt["response_headers"]["retry-after"], "1");
    let receipt_text = serde_json::to_string(&receipt).expect("receipt JSON");
    assert!(!receipt_text.to_ascii_lowercase().contains("authorization"));
    assert!(!receipt_text.contains("local-mock-not-a-secret"));
    assert!(!receipt_text.contains("must-not-be-retained"));
    let balance = audit(&output).expect("round-trip audit");
    assert_eq!(
        balance["outstanding_microusd"],
        report["budget"]["outstanding_microusd"]
    );
    assert_eq!(balance["mode"], "journal_accounting_audit");
    assert_eq!(balance["resume_allowed"], false);
    for (metadata, provenance) in [
        (&report["budget"], "local_mock_http"),
        (&balance, "journal_accounting_audit"),
    ] {
        assert_eq!(metadata["execution_provenance"], provenance);
        assert_eq!(
            metadata["cost_basis"],
            "supplied_tariff_estimate_not_provider_billing"
        );
        assert_eq!(metadata.get("provider_billed_microusd"), Some(&Value::Null));
        assert_eq!(
            metadata["costs_are_tariff_estimates_not_provider_charges"],
            true
        );
    }
    assert_eq!(
        report["paired_bounds"]["treatment_conclusion"],
        "not_generated_by_runner_requires_prespecified_analysis"
    );
    let first = cap_request(&protocol, 0).expect("cap");
    assert_eq!(captures[1].1, first.body);
    // Same initial judgment is retained in each branch; no sibling review is injected.
    let initial = initial_request(&protocol, &cases[0]).expect("initial");
    assert!(captures.iter().any(|(_, body)| body == &initial.body));
    let (base, handle) = server(Vec::new(), output.clone());
    let dispatcher = Dispatcher::new(
        &protocol,
        Some("local-mock-not-a-secret"),
        Some(base),
        Duration::from_secs(1),
    )
    .expect("dispatcher");
    assert!(
        run_source(
            &protocol,
            &cases,
            Source::Http(Box::new(dispatcher)),
            &output
        )
        .is_err(),
        "no resume"
    );
    assert!(handle.join().expect("no HTTP").is_empty());
}

#[test]
fn unknown_count_fee_and_missing_key_refuse_before_any_http() {
    let protocol = policy();
    let mut unknown = protocol.clone();
    unknown.limits.count_request_max_fee_microusd = None;
    // Missing fee is checked before even resolving credentials.
    assert!(
        Dispatcher::from_environment(&unknown)
            .err()
            .expect("refusal")
            .contains("unknown count-request maximum fee")
    );
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
    listener.set_nonblocking(true).expect("nonblocking");
    let base = format!("http://{}/v1", listener.local_addr().expect("address"));
    assert!(
        Dispatcher::new(&protocol, None, Some(base.clone()), TIMEOUT)
            .err()
            .expect("missing key")
            .contains("OPENAI_API_KEY")
    );
    assert!(Dispatcher::new(&unknown, Some("mock"), Some(base), TIMEOUT).is_err());
    assert_eq!(
        listener.accept().err().expect("no requests").kind(),
        std::io::ErrorKind::WouldBlock
    );
    let mut ledger = Ledger::new(unknown.limits).expect("unknown policy representable");
    assert!(ledger.count("one").is_err());
    assert_eq!(ledger.count_attempts(), 0);
}

#[test]
fn execute_cli_refuses_unknown_count_price_without_resolving_credentials() {
    let dir = tempfile::tempdir().expect("temp");
    let protocol_path = dir.path().join("protocol.json");
    let cases_path = dir.path().join("cases.json");
    let output = dir.path().join("never-created");
    let mut protocol = policy();
    protocol.limits.count_request_max_fee_microusd = None;
    protocol.count_tariff = None;
    write_json(&protocol_path, &protocol).expect("unknown-price protocol");
    write_json(&cases_path, &evidence::instrument_cases()).expect("cases");
    let error = run_cli([
        "execute".into(),
        "--protocol".into(),
        protocol_path.display().to_string(),
        "--cases".into(),
        cases_path.display().to_string(),
        "--output".into(),
        output.display().to_string(),
    ])
    .expect_err("unknown price must precede environment resolution");
    assert_eq!(
        error,
        "unknown count-request maximum fee; no HTTP admission"
    );
    assert!(!output.exists());
}

fn supplied_policy() -> Protocol {
    let mut protocol = policy();
    protocol.count_tariff.as_mut().expect("tariff").basis = TariffBasis::SuppliedTariff;
    protocol
}

#[test]
fn credential_environment_wins_without_keychain_lookup() {
    let authorization = resolve_authorization(
        &supplied_policy(),
        || Ok("injected-env-token".into()),
        |_| -> Result<String, hkask_keystore::KeychainError> {
            panic!("environment must win without lookup")
        },
    )
    .expect("environment authorization");
    assert!(authorization.is_sensitive());
    assert_eq!(
        authorization.to_str().expect("header"),
        "Bearer injected-env-token"
    );
}

#[test]
fn credential_invalid_environment_never_falls_back_or_echoes_values() {
    for environment in [
        Ok(String::new()),
        Ok(" injected-secret ".into()),
        Ok("injected-secret\n".into()),
        Ok("injected-secret?".into()),
        Ok("injected-secret:invalid".into()),
        Ok("injected-secreté".into()),
        Err(std::env::VarError::NotUnicode(std::ffi::OsString::from(
            "injected-secret",
        ))),
    ] {
        let error = resolve_authorization(
            &supplied_policy(),
            || environment,
            |_| -> Result<String, hkask_keystore::KeychainError> {
                panic!("configured invalid environment must not fall back")
            },
        )
        .expect_err("invalid environment");
        assert!(error.contains("OPENAI_API_KEY"));
        assert!(!error.contains("injected-secret"));
    }
}

#[test]
fn credential_absent_environment_reads_only_canonical_native_url() {
    let called = std::cell::Cell::new(0);
    let authorization = resolve_authorization(
        &supplied_policy(),
        || Err(std::env::VarError::NotPresent),
        |url| -> Result<String, hkask_keystore::KeychainError> {
            assert_eq!(url, "https://api.openai.com/v1");
            called.set(called.get() + 1);
            Ok("injected-native-token".into())
        },
    )
    .expect("canonical Settings credential");
    assert_eq!(called.get(), 1);
    assert!(authorization.is_sensitive());
    assert_eq!(
        authorization.to_str().expect("header"),
        "Bearer injected-native-token"
    );
}

#[test]
fn credential_keychain_absence_platform_and_timeout_remain_distinct_and_redacted() {
    use hkask_keystore::KeychainError;
    for (failure, classification) in [
        (
            KeychainError::NotFound(hkask_types::NotFound {
                entity_type: "injected-secret".into(),
                id: "injected-secret".into(),
            }),
            "not found",
        ),
        (
            KeychainError::Platform("injected-secret".into()),
            "inaccessible (platform failure)",
        ),
        (
            KeychainError::Timeout {
                key: "injected-secret".into(),
                timeout_secs: 5,
            },
            "timed out",
        ),
    ] {
        let error = resolve_authorization(
            &supplied_policy(),
            || Err(std::env::VarError::NotPresent),
            |_| -> Result<String, KeychainError> { Err(failure) },
        )
        .expect_err("visible credential failure");
        assert!(error.contains(classification), "{error}");
        assert!(error.contains("OPENAI_API_KEY"));
        assert!(error.contains("native OpenAI provider Settings"));
        assert!(!error.contains("injected-secret"));
        if classification != "not found" {
            assert!(!error.contains("not found"));
        }
    }
}

#[test]
fn credential_invalid_keychain_value_is_not_an_absent_credential() {
    for value in [
        "",
        " injected-secret",
        "injected-secret\r\n",
        "injected-secret:invalid",
    ] {
        let error = resolve_authorization(
            &supplied_policy(),
            || Err(std::env::VarError::NotPresent),
            |_| -> Result<String, hkask_keystore::KeychainError> { Ok(value.to_owned()) },
        )
        .expect_err("malformed Settings value");
        assert!(error.contains("native OpenAI provider Settings credential"));
        assert!(error.contains("empty or malformed"));
        assert!(!error.contains("injected-secret"));
        assert!(!error.contains("not found"));
    }
}

#[test]
fn credential_unknown_count_fee_precedes_both_injected_loaders() {
    let mut protocol = supplied_policy();
    protocol.limits.count_request_max_fee_microusd = None;
    let error = resolve_authorization(
        &protocol,
        || -> Result<String, std::env::VarError> {
            panic!("unknown fee must prevent environment resolution")
        },
        |_| -> Result<String, hkask_keystore::KeychainError> {
            panic!("unknown fee must prevent keychain resolution")
        },
    )
    .expect_err("financial refusal first");
    assert_eq!(
        error,
        "unknown count-request maximum fee; no HTTP admission"
    );
}

#[test]
fn review_regression_http_tariffs_cannot_undercut_study_profile() {
    let mut protocol = policy();
    protocol.count_tariff.as_mut().expect("tariff").basis = TariffBasis::SuppliedTariff;
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
    listener.set_nonblocking(true).expect("nonblocking");
    let base = format!("http://{}/v1", listener.local_addr().expect("address"));
    for (input, output) in [(749_999, 4_500_000), (750_000, 4_499_999), (1, 1)] {
        protocol.limits.prompt_microusd_per_million_tokens = input;
        protocol.limits.completion_microusd_per_million_tokens = output;
        for mock in [false, true] {
            assert!(
                validate_policy(&protocol, mock).is_err(),
                "underpriced {input}/{output}, mock={mock}"
            );
        }
        let error = Dispatcher::new(&protocol, None, Some(base.clone()), TIMEOUT)
            .err()
            .expect("tariff refusal precedes even missing-key resolution");
        assert!(error.contains("750000/4500000"), "{error}");
    }
    assert_eq!(
        listener.accept().err().expect("no HTTP").kind(),
        std::io::ErrorKind::WouldBlock
    );
    for (input, output) in [(750_000, 4_500_000), (750_001, 4_500_001)] {
        protocol.limits.prompt_microusd_per_million_tokens = input;
        protocol.limits.completion_microusd_per_million_tokens = output;
        for mock in [false, true] {
            validate_policy(&protocol, mock).expect("exact or conservative higher study rates");
        }
    }
}

#[test]
fn review_regression_every_execution_mode_reports_estimates_not_provider_bills() {
    let mut ledger = Ledger::new(policy().limits).expect("ledger");
    ledger.count("one").expect("count provision");
    ledger.reserve("one", Some(10), false).expect("reserve");
    let response: Value =
        serde_json::from_slice(&generation_reply().bytes).expect("native fixture");
    let (_, usage) =
        native::parse_response(&response, "gpt-5.4-mini-2026-03-17", false).expect("native parse");
    assert!(
        usage
            .as_ref()
            .expect("usage")
            .reported_cost_microusd
            .is_none()
    );
    ledger
        .settle("one", usage.as_ref())
        .expect("tariff settlement");
    for mode in [
        "local_mock_http",
        "live_packet_study",
        "live_not_dispatched",
        "instrument_verification_replay",
        "journal_accounting_audit",
    ] {
        let report = budget_report(&ledger, mode).expect("budget metadata");
        assert_eq!(report["execution_provenance"], mode);
        assert_eq!(
            report["cost_basis"],
            "supplied_tariff_estimate_not_provider_billing"
        );
        assert_eq!(report.get("provider_billed_microusd"), Some(&Value::Null));
        assert_eq!(
            report["costs_are_tariff_estimates_not_provider_charges"],
            true
        );
        assert_eq!(
            report["incurred_microusd"], 53,
            "computed tariff amount, never billed amount"
        );
    }
}

#[test]
fn review_regression_bounds_never_imply_runner_treatment_analysis() {
    assert_eq!(
        bounds(&[])["treatment_conclusion"],
        "not_generated_by_runner_requires_prespecified_analysis"
    );
}

#[test]
fn live_policy_has_no_mock_tariff_or_relaxed_authorization_escape() {
    let protocol = policy();
    assert!(
        validate_policy(&protocol, false)
            .expect_err("mock tariff")
            .contains("mock-only")
    );
    for fault in [
        "source",
        "date",
        "model",
        "attempts",
        "dollars",
        "input",
        "output",
        "provider_max",
        "prompt",
        "completion",
    ] {
        let mut p = protocol.clone();
        p.count_tariff.as_mut().expect("tariff").basis = TariffBasis::SuppliedTariff;
        match fault {
            "source" => p.count_tariff.as_mut().expect("tariff").source.clear(),
            "date" => p.count_tariff.as_mut().expect("tariff").recorded_at.clear(),
            "model" => p.model = "openai/other".into(),
            "attempts" => p.limits.max_attempts = 171,
            "dollars" => p.limits.spending_ceiling_microusd = 10_000_001,
            "input" => p.limits.max_input_tokens = 8193,
            "output" => p.limits.max_output_tokens = 1025,
            "provider_max" => p.limits.provider_output_max = 128001,
            "prompt" => p.limits.prompt_microusd_per_million_tokens = 0,
            "completion" => p.limits.completion_microusd_per_million_tokens = 0,
            _ => panic!("fixture fault"),
        }
        assert!(validate_policy(&p, false).is_err(), "{fault}");
    }
    let mut supplied = protocol;
    supplied.count_tariff.as_mut().expect("tariff").basis = TariffBasis::SuppliedTariff;
    validate_policy(&supplied, false).expect("explicit supplied tariff, not vendor attestation");
    let mut dispatcher = Dispatcher::new(&supplied, Some("mock"), None, TIMEOUT)
        .expect("client construction only; no credential read or provider HTTP");
    assert_eq!(dispatcher.mode(), "live_not_dispatched");
    dispatcher.http_attempts = 1; // Label transition only; not a measurement.
    assert_eq!(dispatcher.mode(), "live_packet_study");
}

#[test]
fn count_http_failures_stop_without_generation_and_keep_count_provision() {
    for fault in [
        "429",
        "redirect",
        "timeout",
        "partial_timeout",
        "partial",
        "malformed",
        "oversized",
        "unknown_count",
        "count_cap",
        "error",
    ] {
        let dir = tempfile::tempdir().expect("temp");
        let output = dir.path().join("run");
        let mut reply = count_reply();
        match fault {
            "429" => {
                reply.status = 429;
                reply.bytes = b"rate limited".to_vec();
            }
            "redirect" => reply.status = 307,
            "timeout" => reply.pause = true,
            "partial_timeout" => {
                reply.bytes = b"{\"raw-prefix\":".to_vec();
                reply.pause = true;
                reply.partial = true;
            }
            "partial" => reply.partial = true,
            "malformed" => reply.bytes = b"raw invalid count".to_vec(),
            "oversized" => reply.bytes = vec![b'x'; MAX_RESPONSE_BYTES + 1],
            "unknown_count" => {
                reply = Reply::json(json!({"object":"response.input_tokens","input_tokens":null}))
            }
            "count_cap" => {
                reply = Reply::json(json!({"object":"response.input_tokens","input_tokens":8193}))
            }
            "error" => {
                reply = Reply::json(
                    json!({"object":"response.input_tokens","input_tokens":10,"error":{"message":"mock error"}}),
                )
            }
            _ => panic!("fixture fault"),
        }
        let prefix = reply.bytes[..reply.bytes.len().min(MAX_RESPONSE_BYTES)].to_vec();
        let timeout = if fault.contains("timeout") {
            Duration::from_millis(70)
        } else {
            Duration::from_secs(3)
        };
        let (report, captures) = run_mock(
            &policy(),
            &evidence::instrument_cases()[..1],
            vec![reply],
            &output,
            timeout,
        );
        assert_eq!(captures.len(), 1, "{fault}");
        assert_eq!(report["budget"]["count_attempts"], 1, "{fault}");
        assert_eq!(report["budget"]["attempts"], 0, "{fault}");
        assert_eq!(report["budget"]["outstanding_count_microusd"], 7, "{fault}");
        assert_eq!(
            report["budget"]["outstanding_generation_microusd"], 0,
            "{fault}"
        );
        assert!(report["budget"]["stopped"].is_string(), "{fault}");
        assert!(!output.join("cap-0.request.json").exists());
        let retained =
            fs::read(output.join("cap-0.count.response.bin")).expect("raw count retained");
        if fault == "timeout" {
            assert!(retained.is_empty());
        } else {
            assert_eq!(retained, prefix, "{fault}");
        }
        assert_eq!(
            audit(&output).expect("audit failure")["outstanding_microusd"],
            7
        );
    }
}

#[test]
fn generation_http_or_native_failures_stop_and_hold_both_liabilities() {
    for fault in [
        "429",
        "redirect",
        "timeout",
        "partial_timeout",
        "model",
        "usage",
        "usage_drift",
        "usage_shape",
        "cap",
        "tool",
        "error",
        "ordinary_incomplete",
    ] {
        let dir = tempfile::tempdir().expect("temp");
        let output = dir.path().join("run");
        let mut generation = generation_reply();
        let mut body: Value = serde_json::from_slice(&generation.bytes).expect("response");
        match fault {
            "429" => generation.status = 429,
            "redirect" => generation.status = 302,
            "timeout" => generation.pause = true,
            "partial_timeout" => {
                generation.bytes = b"{\"raw-prefix\":".to_vec();
                generation.partial = true;
                generation.pause = true;
            }
            "model" => body["model"] = json!("other"),
            "usage" => body
                .as_object_mut()
                .expect("object")
                .remove("usage")
                .map(|_| ())
                .expect("usage field"),
            "usage_drift" => body["usage"]["input_tokens"] = json!(11),
            "usage_shape" => body["usage"] = json!("bad"),
            "cap" => body["usage"]["output_tokens"] = json!(1025),
            "tool" => body["output"][0]["type"] = json!("function_call"),
            "error" => body["error"] = json!({"message":"mock failure"}),
            "ordinary_incomplete" => {
                body["status"] = json!("incomplete");
                body["incomplete_details"] = json!({"reason":"other"});
            }
            _ => panic!("fixture fault"),
        }
        if fault != "partial_timeout" {
            generation.bytes = serde_json::to_vec(&body).expect("response bytes");
        }
        let timeout = if fault.contains("timeout") {
            Duration::from_millis(70)
        } else {
            Duration::from_secs(3)
        };
        let (report, captures) = run_mock(
            &policy(),
            &evidence::instrument_cases()[..1],
            vec![count_reply(), generation],
            &output,
            timeout,
        );
        assert_eq!(captures.len(), 2, "{fault}");
        assert_eq!(report["budget"]["attempts"], 1);
        assert_eq!(report["budget"]["count_attempts"], 1);
        assert_eq!(report["budget"]["outstanding_count_microusd"], 7);
        assert_eq!(
            report["budget"]["outstanding_generation_microusd"], 576008,
            "cap check reserves provider maximum, not requested 1024"
        );
        assert!(report["budget"]["stopped"].is_string(), "{fault}");
        assert!(output.join("cap-0.response.bin").exists());
        let audited = audit(&output).expect("fault audit");
        assert_eq!(
            audited["outstanding_microusd"],
            report["budget"]["outstanding_microusd"]
        );
    }
}

#[test]
fn cap_incomplete_error_null_is_legitimate_but_not_an_ordinary_judgment() {
    let dir = tempfile::tempdir().expect("temp");
    let output = dir.path().join("run");
    let mut protocol = policy();
    protocol.limits.max_attempts = 2;
    let mut body: Value = serde_json::from_slice(&generation_reply().bytes).expect("response");
    body["status"] = json!("incomplete");
    body["incomplete_details"] = json!({"reason":"max_output_tokens"});
    body["output"] = json!([]);
    body["usage"]["output_tokens"] = json!(1024);
    let (report, captures) = run_mock(
        &protocol,
        &evidence::instrument_cases()[..1],
        vec![
            count_reply(),
            Reply::json(body.clone()),
            count_reply(),
            Reply::json(body),
        ],
        &output,
        Duration::from_secs(3),
    );
    assert_eq!(captures.len(), 4);
    assert_eq!(report["budget"]["outstanding_generation_microusd"], 0);
    assert_eq!(report["budget"]["outstanding_count_microusd"], 14);
    assert_eq!(report["budget"]["attempts"], 2);
    assert!(
        report["budget"]["stopped"]
            .as_str()
            .expect("attempt ceiling")
            .contains("count attempt limit")
    );
}

#[test]
fn all_in_boundary_refuses_count_or_generation_before_its_http() {
    for ceiling in [6, 7, 576014, 576015] {
        let dir = tempfile::tempdir().expect("temp");
        let output = dir.path().join("run");
        let mut protocol = policy();
        protocol.limits.spending_ceiling_microusd = ceiling;
        let mut replies = Vec::new();
        if ceiling >= 7 {
            replies.push(count_reply());
        }
        if ceiling == 576015 {
            let mut failed = generation_reply();
            failed.status = 429; // Exact admission boundary followed by sticky stop.
            replies.push(failed);
        }
        let expected_http = replies.len();
        let (report, captures) = run_mock(
            &protocol,
            &evidence::instrument_cases()[..1],
            replies,
            &output,
            Duration::from_secs(3),
        );
        assert_eq!(captures.len(), expected_http, "ceiling {ceiling}");
        assert_eq!(report["budget"]["count_attempts"], u64::from(ceiling >= 7));
        assert_eq!(report["budget"]["attempts"], u64::from(ceiling == 576015));
        assert!(report["budget"]["stopped"].is_string());
        assert_eq!(
            audit(&output).expect("budget audit")["outstanding_microusd"],
            report["budget"]["outstanding_microusd"]
        );
    }
}

#[test]
fn v2_and_unprovisioned_count_journals_are_not_v3_aliases() {
    let mut value = serde_json::to_value(policy()).expect("protocol");
    value["schema"] = json!("packet-study-instrument-v2");
    assert!(
        serde_json::from_value::<Protocol>(value)
            .expect("old shape")
            .validate()
            .is_err()
    );
    assert!(serde_json::from_value::<Event>(json!({"event":"count","id":"old"})).is_err());
    assert!(
        Ledger::replay(
            policy().limits,
            &[Event::Count {
                id: "one".into(),
                max_fee_microusd: 0
            }]
        )
        .is_err(),
        "fee substitution rejected"
    );
}
