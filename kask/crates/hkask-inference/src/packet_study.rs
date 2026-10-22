//! Evidence-packet study controller with native HTTP and digest-bound replay.
//! Replay/mock verification is never a live provider or scientific observation.
//!
//! Replay manifests are JSON arrays of metadata-only `ReplayRecord`s with
//! separate count and generation HTTP fixtures. Each `response_file` is a safe basename relative to the manifest directory; its
//! `response_sha256` binds the source bytes. Bodies are acquired only after a
//! durable reservation and request write. Output manifests reference only the
//! acquired, bounded raw files in the output directory; receipts retain source
//! digests and failure metadata even when only a prefix could be retained.
//! Counters are read after their own synced count event, before generation
//! admission; count-only failures have separate retained replay metadata.

mod budget;
mod evidence;
mod live;
mod native;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use budget::{Event, Ledger, Limits};
use evidence::{Case, CheckerState, Judgment, Method, Verdict, parse_judgment};
use live::Source;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 256 * 1024;
const SCHEMA: &str = "packet-study-instrument-v3";
const GENERATION_ENDPOINT: &str = "https://api.openai.com/v1/responses";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Protocol {
    schema: String,
    model: String,
    schedule_seed: String,
    limits: Limits,
    count_tariff: Option<live::CountTariff>,
}

impl Protocol {
    fn validate(&self) -> Result<(), String> {
        self.limits.validate()?;
        if self.schema != SCHEMA {
            return Err(format!("schema must be {SCHEMA}"));
        }
        let (provider, model) = self
            .model
            .split_once('/')
            .ok_or("explicit provider/model required")?;
        if !provider.eq_ignore_ascii_case("openai")
            || model.is_empty()
            || self.model.chars().any(char::is_whitespace)
        {
            return Err(
                "native request format requires explicit OpenAI/model without whitespace".into(),
            );
        }
        if self.schedule_seed.is_empty() || self.schedule_seed.len() > 128 {
            return Err("schedule_seed must contain 1..=128 bytes".into());
        }
        Ok(())
    }

    fn fingerprint(&self) -> Result<String, String> {
        Ok(digest(
            &serde_json::to_vec(self).map_err(|error| error.to_string())?,
        ))
    }

    fn model_id(&self) -> Result<&str, String> {
        self.model
            .split_once('/')
            .map(|(_, id)| id)
            .ok_or("explicit model required".into())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Stage {
    CapCheck,
    Initial,
    Review,
    Final,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    endpoint: String,
    id: String,
    case_id: Option<String>,
    method: Option<Method>,
    stage: Stage,
    request_sha256: String,
    body: Value,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplayRecord {
    request_sha256: String,
    /// Bound acquired counter metadata; mode/provenance lives in the run report.
    count: Option<CountReplay>,
    response: HttpReplay,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CountReplay {
    generation_body_sha256: String,
    count_request_sha256: String,
    response: HttpReplay,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HttpReplay {
    http_status: u16,
    body_complete: bool,
    response_file: String,
    response_sha256: String,
    transport_error: Option<String>,
}

impl ReplayRecord {
    /// Validate metadata only: admission must precede any response-file access.
    fn validate(&self) -> Result<(), String> {
        validate_digest(&self.request_sha256)?;
        self.response.validate()?;
        if let Some(count) = &self.count {
            validate_digest(&count.generation_body_sha256)?;
            validate_digest(&count.count_request_sha256)?;
            count.response.validate()?;
        }
        Ok(())
    }
}

fn validate_digest(hash: &str) -> Result<(), String> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid replay digest".into());
    }
    Ok(())
}

impl HttpReplay {
    fn validate(&self) -> Result<(), String> {
        validate_digest(&self.response_sha256)?;
        if self.response_file.is_empty()
            || matches!(self.response_file.as_str(), "." | "..")
            || !self
                .response_file
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err("response_file must be a single safe relative basename".into());
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct Branch {
    case_id: String,
    family: String,
    truth: bool,
    primary_state: CheckerState,
    method: Method,
    judgment: Option<Judgment>,
    missing_reason: Option<String>,
}

#[derive(Serialize)]
struct Report {
    schema: &'static str,
    mode: &'static str,
    protocol_sha256: String,
    cases_sha256: String,
    live_execution: &'static str,
    branches: Vec<Branch>,
    paired_bounds: Value,
    budget: Value,
    limitations: &'static str,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JournalEntry {
    seq: usize,
    previous_sha256: String,
    protocol_sha256: String,
    event: Event,
}

struct Journal {
    file: File,
    seq: usize,
    previous: String,
    protocol_sha256: String,
}

impl Journal {
    fn new(path: &Path, protocol: &Protocol) -> Result<Self, String> {
        let protocol_sha256 = protocol.fingerprint()?;
        Ok(Self {
            file: private_file(path)?,
            seq: 0,
            previous: protocol_sha256.clone(),
            protocol_sha256,
        })
    }

    fn append(&mut self, event: Event) -> Result<(), String> {
        let entry = JournalEntry {
            seq: self.seq,
            previous_sha256: self.previous.clone(),
            protocol_sha256: self.protocol_sha256.clone(),
            event,
        };
        let bytes = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        self.file
            .write_all(&bytes)
            .and_then(|()| self.file.write_all(b"\n"))
            .and_then(|()| self.file.sync_all())
            .map_err(|e| format!("journal persist failed: {e}"))?;
        self.previous = digest(&bytes);
        self.seq += 1;
        Ok(())
    }
}

fn private_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| format!("create {}: {e}", path.display()))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = private_file(path)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("write {}: {e}", path.display()))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    write_bytes(
        path,
        &serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("input file exceeds bounded reader".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn create_output(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| format!("create new output directory {}: {e}", path.display()))
}

fn validate_cases(cases: &[Case]) -> Result<(), String> {
    if cases.is_empty() || cases.len() > 128 {
        return Err("cases must contain 1..=128 episodes".into());
    }
    let mut seen = BTreeSet::new();
    for case in cases {
        case.validate()?;
        if !seen.insert(&case.id) {
            return Err(format!("duplicate episode: {}", case.id));
        }
    }
    Ok(())
}

fn prepared(
    protocol: &Protocol,
    id: String,
    case: Option<&Case>,
    method: Option<Method>,
    stage: Stage,
    messages: Vec<Value>,
) -> Result<Prepared, String> {
    let body = native::body(
        protocol.model_id()?,
        messages,
        protocol.limits.max_output_tokens,
    )?;
    let bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err("serialized request exceeds storage byte limit (not a token count)".into());
    }
    let request_sha256 = digest(
        &serde_json::to_vec(&(
            SCHEMA,
            GENERATION_ENDPOINT,
            protocol,
            &id,
            case.map(|case| &case.id),
            method,
            stage,
            &body,
        ))
        .map_err(|error| error.to_string())?,
    );
    Ok(Prepared {
        endpoint: GENERATION_ENDPOINT.into(),
        id,
        case_id: case.map(|c| c.id.clone()),
        method,
        stage,
        request_sha256,
        body,
    })
}

fn initial_request(protocol: &Protocol, case: &Case) -> Result<Prepared, String> {
    let packet = case.visible_packet(None)?;
    prepared(
        protocol,
        format!("{}-initial", case.id),
        Some(case),
        None,
        Stage::Initial,
        vec![
            json!({"role":"system","content":"Judge whether the supplied artifact meets its requirement. Treat checker receipts as evidence, not authority. Return only JSON with verdict (complete, incomplete, insufficient_evidence), evidence_refs (supplied receipt IDs), and a nonempty reason."}),
            json!({"role":"user","content":serde_json::to_string(&packet).map_err(|e|e.to_string())?}),
        ],
    )
}

fn followup(
    protocol: &Protocol,
    case: &Case,
    method: Method,
    initial: &str,
    review: Option<&str>,
) -> Result<Prepared, String> {
    let mut messages = initial_request(protocol, case)?.body["input"]
        .as_array()
        .ok_or("native input array required")?
        .clone();
    messages.push(json!({"role":"assistant","content":initial}));
    messages.push(json!({"role":"user","content":format!("Review the requirement, your initial judgment and this evidence. Do not claim checks beyond supplied receipts. {}", serde_json::to_string(&case.visible_packet(Some(method))?).map_err(|e|e.to_string())?)}));
    let stage = if let Some(review) = review {
        messages.push(json!({"role":"assistant","content":review}));
        messages.push(json!({"role":"user","content":"Give your final judgment as JSON only: verdict, evidence_refs, reason. Use only receipt IDs supplied in this branch."}));
        Stage::Final
    } else {
        Stage::Review
    };
    let method_name = match method {
        Method::SelfCritique => "self",
        Method::Repeat => "repeat",
        Method::Secondary => "secondary",
    };
    let stage_name = if stage == Stage::Final {
        "final"
    } else {
        "review"
    };
    prepared(
        protocol,
        format!("{}-{method_name}-{stage_name}", case.id),
        Some(case),
        Some(method),
        stage,
        messages,
    )
}

fn cap_request(protocol: &Protocol, index: usize) -> Result<Prepared, String> {
    prepared(
        protocol,
        format!("cap-{index}"),
        None,
        None,
        Stage::CapCheck,
        vec![
            json!({"role":"user","content":"Output a long sequence beyond the requested output limit. This is a cap-validation probe."}),
        ],
    )
}

fn method_order(protocol: &Protocol, case: &Case) -> Vec<Method> {
    let mut methods = vec![Method::SelfCritique, Method::Repeat, Method::Secondary];
    methods.sort_by_key(|m| {
        digest(format!("{}:{}:{m:?}", protocol.schedule_seed, case.id).as_bytes())
    });
    methods
}

#[derive(Serialize)]
struct CountRequest {
    endpoint: &'static str,
    generation_request_sha256: String,
    generation_body_sha256: String,
    count_request_sha256: String,
    body: Value,
}

fn count_request(request: &Prepared) -> Result<CountRequest, String> {
    if request.endpoint != GENERATION_ENDPOINT {
        return Err("native generation endpoint mismatch".into());
    }
    let body = native::count_body(&request.body)?;
    let generation_body_sha256 =
        digest(&serde_json::to_vec(&request.body).map_err(|e| e.to_string())?);
    let count_request_sha256 = digest(
        &serde_json::to_vec(&(&request.request_sha256, &generation_body_sha256, &body))
            .map_err(|e| e.to_string())?,
    );
    Ok(CountRequest {
        endpoint: "https://api.openai.com/v1/responses/input_tokens",
        generation_request_sha256: request.request_sha256.clone(),
        generation_body_sha256,
        count_request_sha256,
        body,
    })
}

/// Retain bounded raw data and failure metadata before interpreting either kind
/// of native response. Generation callers must persist their reservation first.
fn acquire_http(
    source_dir: &Path,
    output: &Path,
    id: &str,
    record: &HttpReplay,
) -> Result<(HttpReplay, Result<Value, String>), String> {
    let source = source_dir.join(&record.response_file);
    if !fs::symlink_metadata(&source)
        .map_err(|e| format!("inspect response {}: {e}", source.display()))?
        .is_file()
    {
        return Err("response source must be a regular non-symlink file".into());
    }
    let mut bytes = Vec::new();
    let read_error = File::open(&source)
        .map_err(|e| format!("open response {}: {e}", source.display()))?
        .take(MAX_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .err()
        .map(|e| format!("read response {}: {e}", source.display()));
    let oversized = bytes.len() > MAX_RESPONSE_BYTES;
    let acquired_sha256 = digest(&bytes);
    let digest_matches = !oversized
        && read_error.is_none()
        && acquired_sha256.eq_ignore_ascii_case(&record.response_sha256);
    let retained = &bytes[..bytes.len().min(MAX_RESPONSE_BYTES)];
    let response_file = format!("{id}.response.bin");
    write_bytes(&output.join(&response_file), retained)?;
    let transport_error = read_error
        .clone()
        .or_else(|| record.transport_error.clone())
        .or_else(|| {
            (!digest_matches)
                .then(|| "source response digest mismatch or bounded prefix only".into())
        });
    let acquired = HttpReplay {
        http_status: record.http_status,
        body_complete: record.body_complete && !oversized && digest_matches,
        response_file,
        response_sha256: digest(retained),
        transport_error: transport_error.clone(),
    };
    write_json(
        &output.join(format!("{id}.receipt.json")),
        &json!({
            "mode":"instrument_verification_replay", "http_status":record.http_status,
            "body_complete":acquired.body_complete, "retained_bytes":retained.len(),
            "response_sha256":acquired.response_sha256,
            "source_response_file":record.response_file,"source_response_sha256":record.response_sha256,
            "source_body_complete":record.body_complete,"source_transport_error":record.transport_error,
            "acquired_sha256":acquired_sha256,"acquired_bytes":bytes.len(),
            "source_digest_matches":if oversized || read_error.is_some() {None} else {Some(digest_matches)},
            "oversized":oversized,"read_error":read_error,"transport_error":transport_error
        }),
    )?;
    let parsed = if !acquired.body_complete
        || transport_error.is_some()
        || !(200..300).contains(&record.http_status)
    {
        Err("failed/incomplete HTTP replay; any generation reservation retained".into())
    } else {
        serde_json::from_slice(retained).map_err(|e| format!("malformed native response: {e}"))
    };
    Ok((acquired, parsed))
}

struct Controller<'a> {
    source: Source<'a>,
    ledger: Ledger,
    journal: Journal,
    output: &'a Path,
    acquired: Vec<ReplayRecord>,
}

impl Controller<'_> {
    fn stop(&mut self, reason: &str) -> Result<(), String> {
        self.ledger.stop(reason);
        self.journal.append(Event::Stop {
            reason: reason.into(),
        })
    }

    fn call(&mut self, request: Prepared) -> Result<String, String> {
        let result = self.call_inner(request);
        if let Err(reason) = &result {
            if self.ledger.stopped().is_none() {
                self.stop(reason)?;
            }
        }
        result
    }

    fn call_inner(&mut self, request: Prepared) -> Result<String, String> {
        if let Some(reason) = self.ledger.stopped() {
            return Err(reason.into());
        }
        let frozen = count_request(&request)?;
        self.source.check_binding(&request, &frozen)?;
        let count_id = format!("{}.count", request.id);
        write_json(
            &self.output.join(format!("{count_id}.request.json")),
            &frozen,
        )?;
        self.ledger.count(&request.id)?;
        self.journal.append(Event::Count {
            id: request.id.clone(),
            max_fee_microusd: self.ledger.count_max_fee_microusd()?,
        })?;
        let (count_response, parsed) = self.source.acquire(self.output, &request, Some(&frozen))?;
        let retained_count = CountReplay {
            generation_body_sha256: frozen.generation_body_sha256.clone(),
            count_request_sha256: frozen.count_request_sha256.clone(),
            response: count_response,
        };
        // Count-only failures have their own retained index: do not copy an
        // unadmitted generation's source-body reference into the acquired index.
        write_json(
            &self.output.join(format!("{count_id}.replay.json")),
            &retained_count,
        )?;
        let input_tokens = native::parse_count(&parsed?)?;
        self.ledger.reserve(
            &request.id,
            Some(input_tokens),
            request.stage == Stage::CapCheck,
        )?;
        self.journal.append(Event::Reserve {
            id: request.id.clone(),
            input_upper_bound: Some(input_tokens),
            cap_check: request.stage == Stage::CapCheck,
        })?;
        write_json(
            &self.output.join(format!("{}.request.json", request.id)),
            &request,
        )?;
        // First generation I/O: reservation and exact request are durable.
        let (response, parsed) = self.source.acquire(self.output, &request, None)?;
        self.acquired.push(ReplayRecord {
            request_sha256: request.request_sha256.clone(),
            count: Some(retained_count),
            response,
        });
        let (text, observed) = native::parse_response(
            &parsed?,
            request.body["model"]
                .as_str()
                .ok_or("requested model missing")?,
            request.stage == Stage::CapCheck,
        )?;
        if observed
            .as_ref()
            .and_then(|usage| usage.prompt_tokens)
            .is_some_and(|actual| actual != input_tokens)
        {
            return Err(
                "generation input usage differs from bound native count; reservation retained"
                    .into(),
            );
        }
        // Only a correctly identified, tool-free terminal response may release
        // its allowance (cap-check alone permits cap-induced incompleteness).
        // Malformed final-judgment JSON remains a separate outcome.
        let settled = self.ledger.settle(&request.id, observed.as_ref());
        self.journal.append(Event::Settle {
            id: request.id.clone(),
            usage: observed,
        })?;
        settled?;
        Ok(text)
    }
}

fn budget_report(ledger: &Ledger, mode: &str) -> Result<Value, String> {
    Ok(json!({
        "execution_provenance":mode,
        "cost_basis":"supplied_tariff_estimate_not_provider_billing",
        "provider_billed_microusd":null,
        "costs_are_tariff_estimates_not_provider_charges":true,
        "attempts":ledger.attempts(), "count_attempts":ledger.count_attempts(),
        "http_attempts":ledger.attempts()+ledger.count_attempts(),
        "count_charge_treatment":"maximum fee provision held; not a billing receipt",
        "incurred_microusd":ledger.incurred_microusd(),
        "outstanding_count_microusd":ledger.outstanding_count_microusd(),
        "outstanding_generation_microusd":ledger.outstanding_generation_microusd()?,
        "outstanding_microusd":ledger.outstanding_microusd()?, "stopped":ledger.stopped(),
    }))
}

fn bounds(branches: &[Branch]) -> Value {
    let mut families: BTreeMap<&str, Vec<(f64, f64)>> = BTreeMap::new();
    let mut episodes: BTreeMap<&str, Vec<&Branch>> = BTreeMap::new();
    for branch in branches
        .iter()
        .filter(|b| !b.truth && b.primary_state == CheckerState::Misleading)
    {
        episodes.entry(&branch.case_id).or_default().push(branch);
    }
    for rows in episodes.values() {
        let score = |method| {
            rows.iter()
                .find(|b| b.method == method)
                .and_then(|b| b.judgment.as_ref())
                .map(|j| {
                    if j.verdict == Verdict::Complete {
                        (1.0, 1.0)
                    } else {
                        (0.0, 0.0)
                    }
                })
                .unwrap_or((0.0, 1.0))
        };
        let c = score(Method::Secondary);
        let a = score(Method::SelfCritique);
        families
            .entry(&rows[0].family)
            .or_default()
            .push((c.0 - a.1, c.1 - a.0));
    }
    let intervals=families.iter().map(|(id,pairs)| {
        let n=pairs.len() as f64;
        json!({"family":id,"lower":pairs.iter().map(|p|p.0).sum::<f64>()/n,"upper":pairs.iter().map(|p|p.1).sum::<f64>()/n})
    }).collect::<Vec<_>>();
    let n = intervals.len() as f64;
    json!({"family_count":intervals.len(),"per_family":intervals,
        "lower":if n>0.0 {Some(intervals.iter().filter_map(|v|v["lower"].as_f64()).sum::<f64>()/n)}else{None},
        "upper":if n>0.0 {Some(intervals.iter().filter_map(|v|v["upper"].as_f64()).sum::<f64>()/n)}else{None},
        "is_confidence_interval":false,"treatment_conclusion":"not_generated_by_runner_requires_prespecified_analysis"})
}

fn run(
    protocol: &Protocol,
    cases: &[Case],
    records: &BTreeMap<String, ReplayRecord>,
    source_dir: &Path,
    output: &Path,
) -> Result<Value, String> {
    protocol.validate()?;
    validate_cases(cases)?;
    if records.len() as u64 > protocol.limits.max_attempts {
        return Err("replay record count exceeds attempt ceiling".into());
    }
    for (key, record) in records {
        record.validate()?;
        if key != &record.request_sha256 {
            return Err("replay map key/request digest mismatch".into());
        }
    }
    run_source(
        protocol,
        cases,
        Source::Replay {
            records,
            directory: source_dir,
        },
        output,
    )
}

fn run_source(
    protocol: &Protocol,
    cases: &[Case],
    source: Source<'_>,
    output: &Path,
) -> Result<Value, String> {
    protocol.validate()?;
    validate_cases(cases)?;
    create_output(output)?;
    write_json(&output.join("protocol.json"), protocol)?;
    write_json(&output.join("private-cases.json"), &cases)?;
    write_json(
        &output.join("execution.json"),
        &json!({"source_mode":source.mode(),"resume_allowed":false}),
    )?;

    let mut replay = Controller {
        source,
        ledger: Ledger::new(protocol.limits.clone())?,
        journal: Journal::new(&output.join("budget.jsonl"), protocol)?,
        output,
        acquired: Vec::new(),
    };
    for index in 0..2 {
        if let Err(reason) = replay.call(cap_request(protocol, index)?) {
            replay.stop(&reason)?;
            break;
        }
    }
    let mut ordered = cases.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|c| digest(format!("{}:{}", protocol.schedule_seed, c.id).as_bytes()));
    let mut branches = Vec::new();
    for case in ordered {
        let initial = if replay.ledger.stopped().is_none() {
            replay
                .call(initial_request(protocol, case)?)
                .and_then(|text| {
                    parse_judgment(&text, &case.allowed_receipt_ids(None))?;
                    Ok(text)
                })
        } else {
            Err("earlier instrument failure".into())
        };
        for method in method_order(protocol, case) {
            let judgment = initial.as_ref().map_err(Clone::clone).and_then(|initial| {
                let review = replay.call(followup(protocol, case, method, initial, None)?)?;
                let text =
                    replay.call(followup(protocol, case, method, initial, Some(&review))?)?;
                parse_judgment(&text, &case.allowed_receipt_ids(Some(method)))
            });
            let (judgment, missing_reason) = match judgment {
                Ok(j) => (Some(j), None),
                Err(e) => (None, Some(e)),
            };
            branches.push(Branch {
                case_id: case.id.clone(),
                family: case.family.clone(),
                truth: case.oracle()?,
                primary_state: case.expected_checker_state,
                method,
                judgment,
                missing_reason,
            });
        }
    }
    let mode = replay.source.mode();
    let report = Report {
        schema: SCHEMA,
        mode,
        protocol_sha256: digest(&serde_json::to_vec(protocol).map_err(|e| e.to_string())?),
        cases_sha256: digest(&serde_json::to_vec(cases).map_err(|e| e.to_string())?),
        live_execution: mode,
        paired_bounds: bounds(&branches),
        branches,
        budget: budget_report(&replay.ledger, mode)?,
        limitations: "Count maximum fees remain outstanding, not billed usage. Tariffs are supplied, not authenticated vendor promises. Replay and local HTTP mocks are not provider measurements. Existing controls share a trusted parsing kernel and are not independently calibrated scientific cases; no treatment conclusion follows. store=false is not ZDR. Scientific protocol/readiness and PI interpretation remain outside this CLI.",
    };
    write_json(&output.join("replay-records.json"), &replay.acquired)?;
    write_json(&output.join("report.json"), &report)?;
    Ok(json!({"output":output,"report":report}))
}

fn audit(output: &Path) -> Result<Value, String> {
    let protocol: Protocol = read_json(&output.join("protocol.json"))?;
    protocol.validate()?;
    let mut source = Vec::new();
    File::open(output.join("budget.jsonl"))
        .map_err(|e| e.to_string())?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut source)
        .map_err(|e| e.to_string())?;
    if source.len() as u64 > MAX_FILE_BYTES {
        return Err("oversized journal".into());
    }
    let complete_end = source
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let incomplete_tail_bytes = source.len() - complete_end;
    let prefix = std::str::from_utf8(&source[..complete_end])
        .map_err(|error| format!("corrupt complete journal prefix: {error}"))?;
    let protocol_sha256 = protocol.fingerprint()?;
    let mut previous = protocol_sha256.clone();
    let mut events = Vec::new();
    for (seq, line) in prefix.lines().enumerate() {
        let entry: JournalEntry = serde_json::from_str(line)
            .map_err(|e| format!("corrupt complete journal entry: {e}"))?;
        if entry.seq != seq
            || entry.previous_sha256 != previous
            || entry.protocol_sha256 != protocol_sha256
        {
            return Err("journal chain or protocol mismatch".into());
        }
        previous = digest(&serde_json::to_vec(&entry).map_err(|e| e.to_string())?);
        events.push(entry.event);
    }
    let ledger = Ledger::replay(protocol.limits, &events)?;
    Ok(
        json!({"mode":"journal_accounting_audit","protocol_sha256":protocol_sha256,
        "execution_provenance":"journal_accounting_audit",
        "cost_basis":"supplied_tariff_estimate_not_provider_billing",
        "provider_billed_microusd":null,
        "costs_are_tariff_estimates_not_provider_charges":true,
        "attempts":ledger.attempts(),"count_attempts":ledger.count_attempts(),"http_attempts":ledger.attempts()+ledger.count_attempts(),"count_charge_treatment":"maximum fee provision held; not a billing receipt","incurred_microusd":ledger.incurred_microusd(),
        "outstanding_count_microusd":ledger.outstanding_count_microusd(),
        "outstanding_generation_microusd":ledger.outstanding_generation_microusd()?,
        "outstanding_microusd":ledger.outstanding_microusd()?,"stopped":ledger.stopped(),
        "audit_status":if incomplete_tail_bytes == 0 {"complete_prefix"} else {"degraded_incomplete_tail"},
        "incomplete_tail_bytes":incomplete_tail_bytes,"balance_is_complete":incomplete_tail_bytes == 0,
        "resume_allowed":false,"authenticity":"hash consistency is not producer authentication"}),
    )
}

/// Explicit smoke policy from the reviewed proposal; never a live/model default.
fn smoke_protocol() -> Protocol {
    Protocol {
        schema: SCHEMA.into(),
        model: "openai/gpt-5.4-mini-2026-03-17".into(),
        schedule_seed: "instrument-smoke-only".into(),
        limits: Limits {
            max_attempts: 170,
            count_request_max_fee_microusd: Some(0), // Explicit deterministic mock, not live pricing.
            max_input_tokens: 8192,
            max_output_tokens: 1024,
            provider_output_max: 128000,
            spending_ceiling_microusd: 4_000_000,
            prompt_microusd_per_million_tokens: 750_000,
            completion_microusd_per_million_tokens: 4_500_000,
        },
        count_tariff: Some(live::CountTariff {
            source: "deterministic fixture; no vendor billing claim".into(),
            recorded_at: "instrument-fixture".into(),
            basis: live::TariffBasis::MockOnly,
        }),
    }
}

fn scripted_record(
    source_dir: &Path,
    request: &Prepared,
    text: &str,
) -> Result<ReplayRecord, String> {
    let bytes = serde_json::to_vec(&json!({
        "object":"response", "model":request.body["model"], "status":"completed", "error":null,
        "output":[{"type":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":text}]}],
        "usage":{"input_tokens":10,"output_tokens":10,"output_tokens_details":{"reasoning_tokens":0}}
    })).map_err(|e|e.to_string())?;
    let response_file = format!("{}.response.bin", request.id);
    write_bytes(&source_dir.join(&response_file), &bytes)?;
    Ok(ReplayRecord {
        request_sha256: request.request_sha256.clone(),
        count: Some(scripted_count(source_dir, request, 10)?),
        response: HttpReplay {
            http_status: 200,
            body_complete: true,
            response_file,
            response_sha256: digest(&bytes),
            transport_error: None,
        },
    })
}

fn scripted_count(
    source_dir: &Path,
    request: &Prepared,
    input_tokens: u64,
) -> Result<CountReplay, String> {
    let frozen = count_request(request)?;
    let bytes =
        serde_json::to_vec(&json!({"object":"response.input_tokens","input_tokens":input_tokens}))
            .map_err(|e| e.to_string())?;
    let response_file = format!("{}.count.response.bin", request.id);
    write_bytes(&source_dir.join(&response_file), &bytes)?;
    Ok(CountReplay {
        generation_body_sha256: frozen.generation_body_sha256,
        count_request_sha256: frozen.count_request_sha256,
        response: HttpReplay {
            http_status: 200,
            body_complete: true,
            response_file,
            response_sha256: digest(&bytes),
            transport_error: None,
        },
    })
}

fn smoke_records(
    protocol: &Protocol,
    cases: &[Case],
    source_dir: &Path,
) -> Result<BTreeMap<String, ReplayRecord>, String> {
    create_output(source_dir)?;
    let mut records = BTreeMap::new();
    for index in 0..2 {
        let r = cap_request(protocol, index)?;
        records.insert(
            r.request_sha256.clone(),
            scripted_record(source_dir, &r, "fixture cap response")?,
        );
    }
    for case in cases {
        // Scripted claims deliberately say complete even for bad artifacts. These
        // are labeled instrument controls, not observations from the named model.
        let initial=serde_json::to_string(&json!({"verdict":"complete","evidence_refs":case.allowed_receipt_ids(None),"reason":"scripted instrument initial"})).map_err(|e|e.to_string())?;
        let r = initial_request(protocol, case)?;
        records.insert(
            r.request_sha256.clone(),
            scripted_record(source_dir, &r, &initial)?,
        );
        for method in method_order(protocol, case) {
            let review = "scripted branch review";
            let r = followup(protocol, case, method, &initial, None)?;
            records.insert(
                r.request_sha256.clone(),
                scripted_record(source_dir, &r, review)?,
            );
            let final_text=serde_json::to_string(&json!({"verdict":"complete","evidence_refs":case.allowed_receipt_ids(Some(method)),"reason":"scripted instrument final"})).map_err(|e|e.to_string())?;
            let r = followup(protocol, case, method, &initial, Some(review))?;
            records.insert(
                r.request_sha256.clone(),
                scripted_record(source_dir, &r, &final_text)?,
            );
        }
    }
    Ok(records)
}

/// Standalone CLI entry point, never a GPUI foreground operation. Only execute
/// resolves native OpenAI credentials and creates a current-thread Tokio HTTP runtime.
pub fn run_cli(args: impl IntoIterator<Item = String>) -> Result<Value, String> {
    let mut args = args.into_iter();
    let command=args.next().ok_or("usage: packet-study verify|prepare|replay|audit|execute --output DIR [--protocol JSON --cases JSON --responses JSON]")?;
    let mut options = BTreeMap::new();
    while let Some(key) = args.next() {
        if !["--output", "--protocol", "--cases", "--responses"].contains(&key.as_str()) {
            return Err(format!("unknown option: {key}"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {key}"))?;
        if options.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate option: {key}"));
        }
    }
    if !["verify", "prepare", "replay", "audit", "execute"].contains(&command.as_str()) {
        return Err("unknown mode; use verify/prepare/replay/audit/execute".into());
    }
    let output = PathBuf::from(options.get("--output").ok_or("--output required")?);
    if command == "audit" {
        if options.len() != 1 {
            return Err("audit accepts only --output".into());
        }
        return audit(&output);
    }
    if command == "verify" {
        if options.len() != 1 {
            return Err("verify accepts only --output".into());
        }
        let protocol = smoke_protocol();
        let cases = evidence::instrument_cases();
        if output.exists() {
            return Err("output already exists; never overwrite or automatically resume".into());
        }
        let source_dir = output.with_extension("replay-inputs");
        if source_dir == output {
            return Err("verification output must differ from its replay-inputs sidecar".into());
        }
        let records = smoke_records(&protocol, &cases, &source_dir)?;
        write_json(
            &source_dir.join("replay-records.json"),
            &records.values().collect::<Vec<_>>(),
        )?;
        return run(&protocol, &cases, &records, &source_dir, &output);
    }
    let protocol: Protocol = read_json(Path::new(
        options.get("--protocol").ok_or("--protocol required")?,
    ))?;
    let cases: Vec<Case> = read_json(Path::new(options.get("--cases").ok_or("--cases required")?))?;
    protocol.validate()?;
    validate_cases(&cases)?;
    if command == "execute" {
        if options.len() != 3 {
            return Err("execute requires only --output, --protocol and --cases".into());
        }
        if output.exists() {
            return Err("output already exists; never overwrite or automatically resume".into());
        }
        let dispatcher = live::Dispatcher::from_environment(&protocol)?;
        return run_source(
            &protocol,
            &cases,
            Source::Http(Box::new(dispatcher)),
            &output,
        );
    }
    if command == "prepare" {
        if options.len() != 3 {
            return Err("prepare requires only --output, --protocol and --cases".into());
        }
        create_output(&output)?;
        write_json(&output.join("protocol.json"), &protocol)?;
        write_json(&output.join("private-cases.json"), &cases)?;
        let initial = cases
            .iter()
            .map(|c| initial_request(&protocol, c))
            .collect::<Result<Vec<_>, _>>()?;
        let counts = initial
            .iter()
            .map(count_request)
            .collect::<Result<Vec<_>, _>>()?;
        write_json(&output.join("initial-requests.json"), &initial)?;
        write_json(&output.join("initial-count-requests.json"), &counts)?;
        return Ok(
            json!({"output":output,"live_execution":"blocked","mode":"offline_preparation","initial_requests":initial.len(),"initial_count_requests":counts.len(),"prepared_requests_are_not_dispatch_or_spend_authority":true}),
        );
    }
    if options.len() != 4 {
        return Err("replay requires --output, --protocol, --cases and --responses".into());
    }
    let responses = Path::new(options.get("--responses").ok_or("--responses required")?);
    let records: Vec<ReplayRecord> = read_json(responses)?;
    let source_dir = responses
        .parent()
        .ok_or("responses manifest parent required")?;
    let mut by_digest = BTreeMap::new();
    for record in records {
        record.validate()?;
        if by_digest
            .insert(record.request_sha256.clone(), record)
            .is_some()
        {
            return Err("duplicate replay request digest".into());
        }
    }
    run(&protocol, &cases, &by_digest, source_dir, &output)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Negative controls may overwrite only files in their own fixture directory.
    fn replace_fixture_body(source_dir: &Path, record: &mut ReplayRecord, bytes: &[u8]) {
        assert_eq!(
            source_dir.file_name().and_then(|s| s.to_str()),
            Some("fixture-input")
        );
        fs::write(source_dir.join(&record.response.response_file), bytes)
            .expect("owned fixture replacement");
        record.response.response_sha256 = digest(bytes);
    }

    #[test]
    fn offline_controller_preserves_history_receipts_and_hidden_truth_boundary() {
        let dir = tempfile::tempdir().expect("temp");
        let output = dir.path().join("run");
        let p = smoke_protocol();
        let cases = evidence::instrument_cases();
        let source_dir = dir.path().join("fixture-input");
        let records = smoke_records(&p, &cases, &source_dir).expect("scripted replies");
        let result =
            run(&p, &cases, &records, &source_dir, &output).expect("offline instrument runs");
        assert_eq!(
            result["report"]["branches"]
                .as_array()
                .expect("branches")
                .len(),
            cases.len() * 3
        );
        assert!(result["report"]["budget"]["stopped"].is_null());
        assert_eq!(
            result["report"]["budget"]["attempts"],
            json!(cases.len() * 7 + 2)
        );
        assert_eq!(
            audit(&output).expect("journal replay")["outstanding_microusd"],
            json!(0)
        );
        let c = &cases[0];
        let initial = "retained initial judgment";
        let final_request =
            followup(&p, c, Method::Secondary, initial, Some("own branch only")).expect("request");
        let text = serde_json::to_string(&final_request.body).expect("body");
        assert!(text.contains("retained initial judgment") && text.contains("own branch only"));
        assert!(!text.contains("expected_checker_state") && !text.contains("oracle_truth"));
        assert_eq!(final_request.body["tools"], json!([]));
        assert_eq!(final_request.body["max_output_tokens"], json!(1024));
        assert_eq!(final_request.body["store"], json!(false));
        assert!(final_request.body.get("provider").is_none());
        assert!(
            run(&p, &cases, &records, &source_dir, &output).is_err(),
            "never overwrite or automatically resume"
        );
    }

    #[test]
    fn missing_replay_and_unknown_usage_do_not_become_success_or_live_calls() {
        let dir = tempfile::tempdir().expect("temp");
        let p = smoke_protocol();
        let cases = evidence::instrument_cases();
        let source_dir = dir.path().join("fixture-input");
        let missing = run(
            &p,
            &cases,
            &BTreeMap::new(),
            &source_dir,
            &dir.path().join("missing"),
        )
        .expect("failure report");
        assert_eq!(missing["report"]["budget"]["attempts"], json!(0));
        assert!(
            missing["report"]["branches"]
                .as_array()
                .expect("branches")
                .iter()
                .all(|b| b["judgment"].is_null())
        );
        let mut records = smoke_records(&p, &cases, &source_dir).expect("replay");
        let first = cap_request(&p, 0).expect("cap");
        let record = records.get_mut(&first.request_sha256).expect("first");
        let mut raw: Value =
            read_json(&source_dir.join(&record.response.response_file)).expect("json");
        raw.as_object_mut().expect("object").remove("usage");
        replace_fixture_body(
            &source_dir,
            record,
            &serde_json::to_vec(&raw).expect("bytes"),
        );
        let output = dir.path().join("unknown");
        let result = run(&p, &cases, &records, &source_dir, &output).expect("retained failure");
        assert!(
            result["report"]["budget"]["outstanding_microusd"]
                .as_u64()
                .expect("cost")
                > 0
        );
        assert!(audit(&output).expect("failed settlement retained")["stopped"].is_string());
        assert!(
            run_cli([
                "execute".into(),
                "--output".into(),
                dir.path().join("live").display().to_string()
            ])
            .is_err()
        );
    }

    #[test]
    fn failed_receipts_stop_admission_and_keep_the_first_reservation() {
        for fault in [
            "model",
            "output",
            "tools",
            "finish",
            "http",
            "timeout",
            "incomplete",
            "malformed",
            "oversized",
            "output_cap",
        ] {
            let dir = tempfile::tempdir().expect("temp");
            let protocol = smoke_protocol();
            let cases = evidence::instrument_cases();
            let source_dir = dir.path().join("fixture-input");
            let mut records = smoke_records(&protocol, &cases, &source_dir).expect("records");
            let request = cap_request(&protocol, 0).expect("first");
            let record = records.get_mut(&request.request_sha256).expect("record");
            let mut body: Value =
                read_json(&source_dir.join(&record.response.response_file)).expect("body");
            match fault {
                "model" => body["model"] = json!("unrequested/model"),
                "output" => body["output"] = json!([]),
                "tools" => body["output"][0]["type"] = json!("function_call"),
                "finish" => body["status"] = json!(null),
                "output_cap" => body["usage"]["output_tokens"] = json!(1025),
                "http" => record.response.http_status = 429,
                "timeout" => record.response.transport_error = Some("fixture timeout".into()),
                "incomplete" => record.response.body_complete = false,
                "malformed" | "oversized" => {}
                _ => panic!("known fault"),
            }
            let bytes = match fault {
                "malformed" => b"not JSON".to_vec(),
                "oversized" => vec![b'x'; MAX_RESPONSE_BYTES * 2],
                _ => serde_json::to_vec(&body).expect("encoded body"),
            };
            replace_fixture_body(&source_dir, record, &bytes);
            let output = dir.path().join("run");
            let result = run(&protocol, &cases, &records, &source_dir, &output)
                .expect("failure retained in report");
            assert_eq!(result["report"]["budget"]["attempts"], json!(1), "{fault}");
            assert!(result["report"]["budget"]["stopped"].is_string(), "{fault}");
            assert!(
                result["report"]["budget"]["outstanding_microusd"]
                    .as_u64()
                    .expect("cost")
                    > 0,
                "{fault}: failure must not create spending credit"
            );
            assert!(
                audit(&output).expect("replay failed liability")["outstanding_microusd"]
                    .as_u64()
                    .expect("held")
                    > 0
            );
            if fault == "oversized" {
                assert_eq!(
                    fs::metadata(output.join("cap-0.response.bin"))
                        .expect("partial body")
                        .len(),
                    MAX_RESPONSE_BYTES as u64
                );
                let receipt: Value =
                    read_json(&output.join("cap-0.receipt.json")).expect("receipt");
                assert_eq!(receipt["body_complete"], json!(false));
                assert_eq!(receipt["acquired_bytes"], json!(MAX_RESPONSE_BYTES + 1));
                assert!(
                    receipt["source_digest_matches"].is_null(),
                    "an oversized source digest was not fully checked"
                );
            }
        }
    }

    #[test]
    fn malformed_initial_judgment_retains_every_paired_branch() {
        let dir = tempfile::tempdir().expect("temp");
        let protocol = smoke_protocol();
        let case = evidence::instrument_cases()
            .into_iter()
            .find(|case| {
                !case.oracle().expect("truth")
                    && case.expected_checker_state == CheckerState::Misleading
            })
            .expect("misleading failing control");
        let cases = vec![case];
        let source_dir = dir.path().join("fixture-input");
        let mut records = smoke_records(&protocol, &cases, &source_dir).expect("records");
        let initial = initial_request(&protocol, &cases[0]).expect("initial");
        let record = records
            .get_mut(&initial.request_sha256)
            .expect("initial record");
        let mut body: Value =
            read_json(&source_dir.join(&record.response.response_file)).expect("body");
        body["output"][0]["content"][0]["text"] = json!("not a judgment");
        replace_fixture_body(
            &source_dir,
            record,
            &serde_json::to_vec(&body).expect("bytes"),
        );
        let result = run(
            &protocol,
            &cases,
            &records,
            &source_dir,
            &dir.path().join("run"),
        )
        .expect("missingness report");
        let branches = result["report"]["branches"].as_array().expect("branches");
        assert_eq!(branches.len(), 3);
        assert!(branches.iter().all(|branch| branch["judgment"].is_null()));
        assert_eq!(result["report"]["paired_bounds"]["lower"], json!(-1.0));
        assert_eq!(result["report"]["paired_bounds"]["upper"], json!(1.0));
    }

    #[test]
    fn unknown_bound_and_digest_mismatch_never_acquire_missing_body() {
        for unknown_bound in [true, false] {
            let dir = tempfile::tempdir().expect("temp");
            let source_dir = dir.path().join("fixture-input");
            create_output(&source_dir).expect("separate input directory");
            let protocol = smoke_protocol();
            let request = cap_request(&protocol, 0).expect("cap");
            let key = if unknown_bound {
                request.request_sha256.clone()
            } else {
                "a".repeat(64)
            };
            let record = ReplayRecord {
                request_sha256: key.clone(),
                count: if unknown_bound {
                    None
                } else {
                    Some(scripted_count(&source_dir, &request, 8192).expect("count fixture"))
                },
                response: HttpReplay {
                    http_status: 200,
                    body_complete: true,
                    response_file: "absent-sentinel.bin".into(),
                    response_sha256: digest(b"unopened sentinel"),
                    transport_error: None,
                },
            };
            let records = BTreeMap::from([(key, record)]);
            let output = dir.path().join("run");
            let result = run(
                &protocol,
                &evidence::instrument_cases(),
                &records,
                &source_dir,
                &output,
            )
            .expect("admission refusal is reported, not a failed file lookup");
            assert_eq!(result["report"]["budget"]["attempts"], json!(0));
            let reason = result["report"]["budget"]["stopped"]
                .as_str()
                .expect("stop");
            assert!(reason.contains(if unknown_bound {
                "missing native count replay"
            } else {
                "request-digest replay mismatch"
            }));
            let index: Vec<ReplayRecord> =
                read_json(&output.join("replay-records.json")).expect("index");
            assert!(index.is_empty());
            assert!(!output.join("cap-0.request.json").exists());
            assert!(fs::read_dir(&output).expect("output").all(|entry| {
                !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".response.bin")
            }));
        }
    }

    #[test]
    fn replay_metadata_rejects_unsafe_references_and_inline_bodies_without_lookup() {
        let dir = tempfile::tempdir().expect("temp");
        let protocol = smoke_protocol();
        let request = cap_request(&protocol, 0).expect("cap");
        let mut record = ReplayRecord {
            request_sha256: request.request_sha256.clone(),
            count: None,
            response: HttpReplay {
                http_status: 200,
                body_complete: true,
                response_file: "absent.bin".into(),
                response_sha256: digest(b"absent"),
                transport_error: None,
            },
        };
        record
            .validate()
            .expect("metadata validation does not require the body to exist");
        for path in [
            "../outside.bin",
            "/absolute.bin",
            "nested/body.bin",
            "..",
            ".",
            "",
            "back\\slash.bin",
        ] {
            record.response.response_file = path.into();
            let records = BTreeMap::from([(request.request_sha256.clone(), record.clone())]);
            let output = dir.path().join("never-created");
            let error = run(
                &protocol,
                &evidence::instrument_cases(),
                &records,
                &dir.path().join("absent"),
                &output,
            )
            .expect_err("unsafe reference");
            assert!(error.contains("safe relative basename"));
            assert!(!output.exists());
        }
        record.response.response_file = "absent.bin".into();
        record.response.response_sha256 = "not-a-digest".into();
        assert!(record.validate().is_err());
        record.response.response_sha256 = digest(b"absent");
        let mut inline = serde_json::to_value(&record).expect("metadata");
        inline["response_body"] = json!([1, 2, 3]);
        assert!(
            serde_json::from_value::<ReplayRecord>(inline).is_err(),
            "old eager-body format must fail visibly"
        );
    }

    #[test]
    fn absent_admitted_body_is_looked_up_only_after_durable_reserve_and_request() {
        let dir = tempfile::tempdir().expect("temp");
        let source_dir = dir.path().join("fixture-input");
        create_output(&source_dir).expect("input");
        let protocol = smoke_protocol();
        let request = cap_request(&protocol, 0).expect("cap");
        let record = ReplayRecord {
            request_sha256: request.request_sha256.clone(),
            count: Some(scripted_count(&source_dir, &request, 8192).expect("counter fixture")),
            response: HttpReplay {
                http_status: 200,
                body_complete: true,
                response_file: "absent.bin".into(),
                response_sha256: digest(b"absent"),
                transport_error: None,
            },
        };
        let output = dir.path().join("run");
        let records = BTreeMap::from([(request.request_sha256.clone(), record)]);
        let result = run(
            &protocol,
            &evidence::instrument_cases(),
            &records,
            &source_dir,
            &output,
        )
        .expect("failure report");
        assert_eq!(result["report"]["budget"]["attempts"], json!(1));
        assert!(
            result["report"]["budget"]["stopped"]
                .as_str()
                .expect("reason")
                .contains("absent.bin")
        );
        let persisted: Prepared =
            read_json(&output.join("cap-0.request.json")).expect("request before lookup");
        assert_eq!(persisted.request_sha256, request.request_sha256);
        let audited = audit(&output).expect("durable held reservation");
        assert!(audited["outstanding_microusd"].as_u64().expect("held") > 0);
        let index: Vec<ReplayRecord> =
            read_json(&output.join("replay-records.json")).expect("index");
        assert!(index.is_empty());
        assert!(!output.join("cap-0.response.bin").exists());
    }

    #[test]
    fn object_shaped_final_verdict_is_missing_not_complete_or_abstention() {
        let dir = tempfile::tempdir().expect("temp");
        let source_dir = dir.path().join("fixture-input");
        let protocol = smoke_protocol();
        let cases = vec![evidence::instrument_cases().remove(0)];
        let mut records = smoke_records(&protocol, &cases, &source_dir).expect("records");
        let initial = serde_json::to_string(&json!({"verdict":"complete","evidence_refs":cases[0].allowed_receipt_ids(None),"reason":"scripted instrument initial"})).expect("initial");
        let request = followup(
            &protocol,
            &cases[0],
            Method::Secondary,
            &initial,
            Some("scripted branch review"),
        )
        .expect("final");
        let record = records
            .get_mut(&request.request_sha256)
            .expect("final record");
        let mut body: Value =
            read_json(&source_dir.join(&record.response.response_file)).expect("body");
        body["output"][0]["content"][0]["text"] = json!(serde_json::to_string(&json!({
            "verdict":{"complete":null},"evidence_refs":cases[0].allowed_receipt_ids(Some(Method::Secondary)),"reason":"object verdict"
        })).expect("malformed judgment"));
        replace_fixture_body(
            &source_dir,
            record,
            &serde_json::to_vec(&body).expect("bytes"),
        );
        let result = run(
            &protocol,
            &cases,
            &records,
            &source_dir,
            &dir.path().join("run"),
        )
        .expect("report");
        let branch = result["report"]["branches"]
            .as_array()
            .expect("branches")
            .iter()
            .find(|branch| branch["method"] == json!(Method::Secondary))
            .expect("secondary");
        assert!(branch["judgment"].is_null());
        assert!(branch["missing_reason"].is_string());
        assert!(
            result["report"]["budget"]["stopped"].is_null(),
            "judgment missingness is not a transport failure"
        );
    }

    #[test]
    fn retained_index_roundtrips_and_binds_source_digest_mismatch() {
        let dir = tempfile::tempdir().expect("temp");
        let source_dir = dir.path().join("fixture-input");
        let protocol = smoke_protocol();
        let cases = evidence::instrument_cases();
        let mut records = smoke_records(&protocol, &cases, &source_dir).expect("records");
        let first = cap_request(&protocol, 0).expect("cap");
        records
            .get_mut(&first.request_sha256)
            .expect("first")
            .response
            .response_sha256 = "0".repeat(64);
        let output = dir.path().join("run");
        let result =
            run(&protocol, &cases, &records, &source_dir, &output).expect("mismatch report");
        assert_eq!(result["report"]["budget"]["attempts"], json!(1));
        let receipt: Value = read_json(&output.join("cap-0.receipt.json")).expect("receipt");
        assert_eq!(receipt["source_response_sha256"], json!("0".repeat(64)));
        assert_eq!(receipt["source_digest_matches"], json!(false));
        let index: Vec<ReplayRecord> =
            read_json(&output.join("replay-records.json")).expect("index");
        assert_eq!(index.len(), 1, "unadmitted bodies are not archived");
        assert!(!output.join("cap-1.response.bin").exists());
        let replayed = run_cli([
            "replay".into(),
            "--output".into(),
            dir.path().join("replayed").display().to_string(),
            "--protocol".into(),
            output.join("protocol.json").display().to_string(),
            "--cases".into(),
            output.join("private-cases.json").display().to_string(),
            "--responses".into(),
            output.join("replay-records.json").display().to_string(),
        ])
        .expect("retained failure replays");
        assert_eq!(result["report"]["budget"], replayed["report"]["budget"]);
    }

    #[test]
    fn replay_fingerprint_binds_identical_bodies_to_distinct_attempts_and_policy() {
        let protocol = smoke_protocol();
        let first = cap_request(&protocol, 0).expect("first cap request");
        let second = cap_request(&protocol, 1).expect("second cap request");
        assert_eq!(
            first.body, second.body,
            "same wire body is a legitimate repeated request"
        );
        assert_ne!(
            first.request_sha256, second.request_sha256,
            "a body-only hash would silently reuse the cap receipt"
        );
        let mut changed = protocol;
        changed.limits.spending_ceiling_microusd += 1;
        assert_ne!(
            first.request_sha256,
            cap_request(&changed, 0)
                .expect("changed policy")
                .request_sha256
        );
    }

    #[test]
    fn journal_torn_tail_preserves_durable_prefix_liability() {
        let dir = tempfile::tempdir().expect("temp");
        let output = dir.path().join("run");
        create_output(&output).expect("new run");
        let protocol = smoke_protocol();
        write_json(&output.join("protocol.json"), &protocol).expect("protocol");
        let path = output.join("budget.jsonl");
        let mut journal = Journal::new(&path, &protocol).expect("journal");
        journal
            .append(Event::Reserve {
                id: "attempt".into(),
                input_upper_bound: Some(8192),
                cap_check: true,
            })
            .expect("durable reservation");
        drop(journal);
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("append partial event");
        file.write_all(b"{\"seq\":1").expect("torn tail");
        file.sync_all().expect("persist tail");
        let result = audit(&output).expect("recover known prefix despite torn tail");
        assert_eq!(result["outstanding_microusd"], json!(582144));
        assert_eq!(result["balance_is_complete"], json!(false));
        assert_eq!(result["resume_allowed"], json!(false));
        assert!(result["incomplete_tail_bytes"].as_u64().expect("tail") > 0);
        file.write_all(b"}\n").expect("complete but corrupt event");
        file.sync_all().expect("persist corruption");
        assert!(
            audit(&output).is_err(),
            "corrupt complete entries are not a torn tail"
        );
    }

    #[test]
    fn journal_rejects_protocol_substitution_before_reconstructing_balances() {
        let dir = tempfile::tempdir().expect("temp");
        let output = dir.path().join("run");
        create_output(&output).expect("new run");
        let mut protocol = smoke_protocol();
        write_json(&output.join("protocol.json"), &protocol).expect("protocol");
        let mut journal = Journal::new(&output.join("budget.jsonl"), &protocol).expect("journal");
        journal
            .append(Event::Reserve {
                id: "attempt".into(),
                input_upper_bound: Some(8192),
                cap_check: true,
            })
            .expect("reserved");
        assert_eq!(
            audit(&output).expect("original protocol")["outstanding_microusd"],
            json!(582144)
        );
        protocol.limits.prompt_microusd_per_million_tokens = 0;
        protocol.limits.completion_microusd_per_million_tokens = 0;
        fs::write(
            output.join("protocol.json"),
            serde_json::to_vec(&protocol).expect("protocol bytes"),
        )
        .expect("test policy replacement");
        assert!(
            audit(&output).is_err(),
            "a cheaper substituted policy must not erase held liability"
        );
    }

    #[test]
    fn count_failures_stop_before_generation_reservation_or_body_access() {
        for fault in [
            "missing",
            "binding",
            "count_binding",
            "http",
            "timeout",
            "partial",
            "malformed",
            "object",
            "null",
            "over_limit",
            "digest",
            "oversized",
        ] {
            let dir = tempfile::tempdir().expect("temp");
            let source = dir.path().join("fixture-input");
            let protocol = smoke_protocol();
            let cases = evidence::instrument_cases();
            let mut records = smoke_records(&protocol, &cases, &source).expect("fixtures");
            let first = cap_request(&protocol, 0).expect("request");
            let record = records.get_mut(&first.request_sha256).expect("first");
            // If generation is reached, this missing sentinel makes it visible.
            record.response.response_file = "generation-must-not-be-opened.bin".into();
            let count = record.count.as_mut().expect("count fixture");
            let mut body = json!({"object":"response.input_tokens","input_tokens":10});
            match fault {
                "missing" => {}
                "binding" => count.generation_body_sha256 = "0".repeat(64),
                "count_binding" => count.count_request_sha256 = "0".repeat(64),
                "http" => count.response.http_status = 429,
                "timeout" => count.response.transport_error = Some("scripted timeout".into()),
                "partial" => count.response.body_complete = false,
                "object" => body["object"] = json!("wrong"),
                "null" => body["input_tokens"] = Value::Null,
                "over_limit" => body["input_tokens"] = json!(8193),
                "malformed" | "digest" | "oversized" => {}
                _ => panic!("known fault"),
            }
            let bytes = match fault {
                "malformed" => b"not JSON".to_vec(),
                "oversized" => vec![b'x'; MAX_RESPONSE_BYTES + 20],
                _ => serde_json::to_vec(&body).expect("bytes"),
            };
            fs::write(source.join(&count.response.response_file), &bytes)
                .expect("owned count fixture");
            count.response.response_sha256 = if fault == "digest" {
                "0".repeat(64)
            } else {
                digest(&bytes)
            };
            if fault == "missing" {
                record.count = None;
            }
            let output = dir.path().join("run");
            let result =
                run(&protocol, &cases, &records, &source, &output).expect("failure report");
            let report = &result["report"]["budget"];
            let count_attempts =
                u64::from(!matches!(fault, "missing" | "binding" | "count_binding"));
            assert_eq!(report["count_attempts"], count_attempts, "{fault}");
            assert_eq!(report["attempts"], 0, "{fault}");
            assert_eq!(
                report["outstanding_microusd"], 0,
                "no generation liability: {fault}"
            );
            assert_eq!(
                report["count_charge_treatment"],
                "maximum fee provision held; not a billing receipt"
            );
            assert!(report["stopped"].is_string(), "{fault}");
            assert!(
                !report["stopped"]
                    .as_str()
                    .expect("reason")
                    .contains("generation-must-not-be-opened")
            );
            assert!(!output.join("cap-0.request.json").exists());
            assert!(!output.join("cap-0.response.bin").exists());
            let audited = audit(&output).expect("count-only audit");
            assert_eq!(audited["count_attempts"], count_attempts);
            assert_eq!(audited["attempts"], 0);
            if count_attempts == 1 {
                assert!(output.join("cap-0.count.response.bin").exists(), "{fault}");
                assert!(output.join("cap-0.count.replay.json").exists(), "{fault}");
            }
        }
    }

    #[test]
    fn generation_usage_must_match_count_and_retains_liability_on_drift() {
        let dir = tempfile::tempdir().expect("temp");
        let source = dir.path().join("fixture-input");
        let protocol = smoke_protocol();
        let cases = evidence::instrument_cases();
        let mut records = smoke_records(&protocol, &cases, &source).expect("fixtures");
        let first = cap_request(&protocol, 0).expect("request");
        let record = records.get_mut(&first.request_sha256).expect("first");
        let mut body: Value =
            read_json(&source.join(&record.response.response_file)).expect("body");
        body["usage"]["input_tokens"] = json!(9);
        replace_fixture_body(&source, record, &serde_json::to_vec(&body).expect("bytes"));
        let output = dir.path().join("run");
        let result = run(&protocol, &cases, &records, &source, &output).expect("report");
        assert_eq!(result["report"]["budget"]["attempts"], 1);
        assert_eq!(result["report"]["budget"]["count_attempts"], 1);
        assert!(
            result["report"]["budget"]["stopped"]
                .as_str()
                .expect("stopped")
                .contains("differs from bound native count")
        );
        assert!(
            audit(&output).expect("audit")["outstanding_microusd"]
                .as_u64()
                .expect("liability")
                > 0
        );
    }

    #[test]
    fn count_binding_detects_all_generation_changes_and_cross_attempt_reuse() {
        let protocol = smoke_protocol();
        let first = cap_request(&protocol, 0).expect("first");
        let original = count_request(&first).expect("count");
        let second = count_request(&cap_request(&protocol, 1).expect("second")).expect("count");
        assert_eq!(
            original.generation_body_sha256,
            second.generation_body_sha256
        );
        assert_ne!(original.count_request_sha256, second.count_request_sha256);
        for field in ["input", "model", "max_output_tokens"] {
            let mut changed = first.clone();
            changed.body[field] = match field {
                "input" => json!([{"role":"user","content":"different retained history"}]),
                "model" => json!("different-snapshot"),
                _ => json!(1023),
            };
            let recomputed = count_request(&changed).expect("valid changed request");
            assert_ne!(
                original.generation_body_sha256,
                recomputed.generation_body_sha256
            );
            assert_ne!(
                original.count_request_sha256,
                recomputed.count_request_sha256
            );
        }
        let mut changed = first.clone();
        changed.endpoint = "https://unapproved.example/responses".into();
        assert!(count_request(&changed).is_err());
        let mut changed = first;
        changed.body["previous_response_id"] = json!("forbidden-server-state");
        assert!(count_request(&changed).is_err());
    }

    #[test]
    fn native_cap_truncation_is_not_an_ordinary_judgment_or_failed_http_body() {
        let dir = tempfile::tempdir().expect("temp");
        let source = dir.path().join("fixture-input");
        let protocol = smoke_protocol();
        let cases = evidence::instrument_cases();
        let mut records = smoke_records(&protocol, &cases, &source).expect("fixtures");
        let first = cap_request(&protocol, 0).expect("request");
        let record = records.get_mut(&first.request_sha256).expect("first");
        let mut body: Value =
            read_json(&source.join(&record.response.response_file)).expect("body");
        body["status"] = json!("incomplete");
        body["incomplete_details"] = json!({"reason":"max_output_tokens"});
        body["output"] = json!([]);
        replace_fixture_body(&source, record, &serde_json::to_vec(&body).expect("bytes"));
        let result = run(
            &protocol,
            &cases,
            &records,
            &source,
            &dir.path().join("run"),
        )
        .expect("report");
        assert!(result["report"]["budget"]["stopped"].is_null());
        assert_eq!(result["report"]["budget"]["outstanding_microusd"], 0);
    }

    #[test]
    fn prepare_exports_paired_native_endpoints_and_bound_count_projection() {
        let dir = tempfile::tempdir().expect("temp");
        let protocol_path = dir.path().join("protocol.json");
        let cases_path = dir.path().join("cases.json");
        let output = dir.path().join("prepared");
        write_json(&protocol_path, &smoke_protocol()).expect("protocol");
        write_json(&cases_path, &evidence::instrument_cases()).expect("cases");
        let result = run_cli([
            "prepare".into(),
            "--protocol".into(),
            protocol_path.display().to_string(),
            "--cases".into(),
            cases_path.display().to_string(),
            "--output".into(),
            output.display().to_string(),
        ])
        .expect("offline preparation");
        assert_eq!(result["initial_requests"], result["initial_count_requests"]);
        let generations: Vec<Prepared> =
            read_json(&output.join("initial-requests.json")).expect("generations");
        let counts: Vec<Value> =
            read_json(&output.join("initial-count-requests.json")).expect("counts");
        assert_eq!(generations.len(), counts.len());
        for (generation, count) in generations.iter().zip(counts) {
            assert_eq!(generation.endpoint, GENERATION_ENDPOINT);
            assert_eq!(
                count["endpoint"],
                "https://api.openai.com/v1/responses/input_tokens"
            );
            assert_eq!(
                count["generation_request_sha256"],
                generation.request_sha256
            );
            assert_eq!(
                count["body"],
                native::count_body(&generation.body).expect("projection")
            );
            assert_eq!(
                count,
                serde_json::to_value(count_request(generation).expect("binding")).expect("json")
            );
        }
        assert!(
            !output.join("budget.jsonl").exists(),
            "preparation is not a dispatch"
        );
    }

    #[test]
    fn v1_protocol_and_legacy_whole_token_tariffs_fail_instead_of_reinterpreting() {
        let mut value = serde_json::to_value(smoke_protocol()).expect("protocol");
        value["schema"] = json!("packet-study-instrument-v1");
        assert!(
            serde_json::from_value::<Protocol>(value.clone())
                .expect("shape")
                .validate()
                .is_err()
        );
        value["schema"] = json!(SCHEMA);
        value["limits"]["prompt_microusd_per_token"] = json!(2);
        assert!(serde_json::from_value::<Protocol>(value).is_err());
    }

    #[test]
    fn missing_pair_members_produce_bounds_not_denominator_dropping() {
        let branches = vec![Branch {
            case_id: "e".into(),
            family: "f".into(),
            truth: false,
            primary_state: CheckerState::Misleading,
            method: Method::SelfCritique,
            judgment: None,
            missing_reason: Some("initial failed".into()),
        }];
        let result = bounds(&branches);
        assert_eq!(result["family_count"], json!(1));
        assert_eq!(result["lower"], json!(-1.0));
        assert_eq!(result["upper"], json!(1.0));
    }
}
