//! Blossom HTTP/1.1 listener (`BUD-01` / `BUD-02` / `BUD-06`).
//!
//! Serves streaming binary blobs on `127.0.0.1:<port>` with `Access-Control-Allow-Origin: *`
//! for media playback, HTTP `Range` support for MP4 seeking, and Schnorr-verified
//! `kind: 24242` auth for `PUT /upload`, `HEAD /upload`, and `DELETE /<sha256>`.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use hkask_types::nostr_wire::MAX_BLOSSOM_UPLOAD_BYTES;
use http_body::{Body, Frame};
use hyper::body::Incoming;
use hyper::header::{
    ACCEPT_RANGES, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_EXPOSE_HEADERS, AUTHORIZATION, CONTENT_DISPOSITION,
    CONTENT_LENGTH, CONTENT_RANGE, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, HeaderValue, RANGE,
    X_CONTENT_TYPE_OPTIONS,
};
use hyper::server::conn::http1;
use hyper::service::Service;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncSeekExt, AsyncWriteExt, ReadBuf};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

use crate::auth::{AuthAction, is_valid_sha256_hex, verify_blossom_auth};
use crate::config::parse_pubkey;
use crate::server::BlossomState;
use crate::storage::{BlobRecord, DeleteOutcome, StorageError};

const MAX_HTTP_CONNECTIONS: usize = 128;
const MAX_CONCURRENT_UPLOADS: usize = 16;
const UPLOAD_BODY_TIMEOUT: Duration = Duration::from_secs(120);
const HTTP_CONNECTION_TIMEOUT: Duration = Duration::from_secs(120);

pub enum BlossomBody {
    Full(Option<Bytes>),
    File {
        file: tokio::fs::File,
        remaining: u64,
    },
}

impl BlossomBody {
    fn empty() -> Self {
        Self::Full(None)
    }

    fn from_bytes(bytes: impl Into<Bytes>) -> Self {
        let b = bytes.into();
        if b.is_empty() {
            Self::Full(None)
        } else {
            Self::Full(Some(b))
        }
    }
}

impl Body for BlossomBody {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.get_mut() {
            Self::Full(slot) => match slot.take() {
                Some(b) => Poll::Ready(Some(Ok(Frame::data(b)))),
                None => Poll::Ready(None),
            },
            Self::File { file, remaining } => {
                if *remaining == 0 {
                    return Poll::Ready(None);
                }
                let to_read = (*remaining as usize).min(65_536);
                let mut buf = vec![0u8; to_read];
                let mut read_buf = ReadBuf::new(&mut buf);
                match Pin::new(file).poll_read(cx, &mut read_buf) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(err)) => {
                        *remaining = 0;
                        Poll::Ready(Some(Err(err)))
                    }
                    Poll::Ready(Ok(())) => {
                        let filled = read_buf.filled().len();
                        if filled == 0 {
                            *remaining = 0;
                            Poll::Ready(None)
                        } else {
                            *remaining -= filled as u64;
                            buf.truncate(filled);
                            Poll::Ready(Some(Ok(Frame::data(Bytes::from(buf)))))
                        }
                    }
                }
            }
        }
    }
}

fn add_cors_headers(resp: &mut Response<BlossomBody>) {
    let h = resp.headers_mut();
    h.insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, PUT, DELETE, OPTIONS"),
    );
    h.insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(
            "Authorization, Content-Type, X-SHA-256, X-Content-Length, X-Content-Type",
        ),
    );
    h.insert(
        ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("X-Reason"),
    );
}

fn json_response(status: StatusCode, body: serde_json::Value) -> Response<BlossomBody> {
    let mut resp = Response::new(BlossomBody::from_bytes(body.to_string()));
    *resp.status_mut() = status;
    resp.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    add_cors_headers(&mut resp);
    resp
}

fn error_response(status: StatusCode, reason: &str) -> Response<BlossomBody> {
    let mut resp = Response::new(BlossomBody::from_bytes(
        json!({ "error": reason }).to_string(),
    ));
    *resp.status_mut() = status;
    resp.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Ok(val) = HeaderValue::from_str(reason) {
        resp.headers_mut().insert("x-reason", val);
    }
    add_cors_headers(&mut resp);
    resp
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn header_str<'a>(req: &'a Request<Incoming>, name: &str) -> Option<&'a str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

fn host_allowed(req: &Request<Incoming>, port: u16, public_host: Option<&str>) -> bool {
    let Some(host) = req.headers().get(HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let lower = host.trim().to_ascii_lowercase();
    if lower == format!("127.0.0.1:{port}") || lower == format!("localhost:{port}") {
        return true;
    }
    if let Some(pub_host) = public_host {
        return lower == pub_host;
    }
    false
}

fn admitted_server_domains(port: u16, public_host: Option<&str>) -> Vec<String> {
    let mut domains = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if let Some(pub_host) = public_host {
        domains.push(pub_host.to_ascii_lowercase());
    }
    domains
}

fn is_passive_media_mime(mime: &str) -> bool {
    let base = mime.split(';').next().unwrap_or(mime).trim();
    matches!(
        base.to_ascii_lowercase().as_str(),
        "video/mp4"
            | "video/quicktime"
            | "video/webm"
            | "video/ogg"
            | "image/jpeg"
            | "image/png"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
            | "audio/mpeg"
            | "audio/wav"
            | "audio/x-wav"
            | "audio/wave"
            | "audio/ogg"
            | "audio/mp4"
            | "audio/x-m4a"
            | "audio/aac"
            | "audio/flac"
            | "audio/webm"
    )
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "video/mp4" => ".mp4",
        "video/quicktime" => ".mov",
        "video/webm" => ".webm",
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "audio/mpeg" => ".mp3",
        "audio/wav" => ".wav",
        "audio/ogg" => ".ogg",
        "audio/mp4" => ".m4a",
        _ => "",
    }
}

fn blob_descriptor_json(record: &BlobRecord, base_url: &str) -> serde_json::Value {
    let ext = ext_for_mime(&record.mime_type);
    json!({
        "url": format!("{base_url}/{}{ext}", record.sha256),
        "sha256": record.sha256,
        "size": record.size,
        "type": record.mime_type,
        "uploaded": record.uploaded_at,
    })
}

/// Parse `bytes=<start>-<end>` (single range).
fn parse_byte_range(header: &str, total: u64) -> Result<(u64, u64), ()> {
    if total == 0 {
        return Err(());
    }
    let spec = header.trim().strip_prefix("bytes=").ok_or(())?;
    if spec.contains(',') {
        return Err(());
    }
    let (start_str, end_str) = spec.split_once('-').ok_or(())?;
    if start_str.is_empty() {
        let suffix: u64 = end_str.trim().parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        let start = total.saturating_sub(suffix);
        return Ok((start, total - 1));
    }
    let start: u64 = start_str.trim().parse().map_err(|_| ())?;
    if start >= total {
        return Err(());
    }
    let end = if end_str.trim().is_empty() {
        total - 1
    } else {
        let e: u64 = end_str.trim().parse().map_err(|_| ())?;
        e.min(total - 1)
    };
    if start > end {
        return Err(());
    }
    Ok((start, end))
}

fn extract_sha256_segment(path: &str) -> Option<&str> {
    let seg = path.strip_prefix('/')?;
    if seg.is_empty() || seg.contains('/') {
        return None;
    }
    let stem = seg.split('.').next()?;
    if is_valid_sha256_hex(stem) {
        Some(stem)
    } else {
        None
    }
}

async fn handle_request(
    state: Arc<BlossomState>,
    port: u16,
    upload_sem: Arc<Semaphore>,
    req: Request<Incoming>,
) -> Response<BlossomBody> {
    if req.method() == Method::OPTIONS {
        let mut resp = Response::new(BlossomBody::empty());
        *resp.status_mut() = StatusCode::NO_CONTENT;
        add_cors_headers(&mut resp);
        return resp;
    }

    if !host_allowed(&req, port, state.config.public_host.as_deref()) {
        return error_response(StatusCode::MISDIRECTED_REQUEST, "unexpected host");
    }

    let admitted_domains = admitted_server_domains(port, state.config.public_host.as_deref());
    let admitted_refs: Vec<&str> = admitted_domains.iter().map(String::as_str).collect();

    let request_base_url = match (
        state.config.public_base_url.as_deref(),
        state.config.public_host.as_deref(),
        req.headers().get(HOST).and_then(|v| v.to_str().ok()),
    ) {
        (Some(pub_url), Some(pub_host), Some(h)) if h.trim().eq_ignore_ascii_case(pub_host) => {
            pub_url.to_string()
        }
        _ => format!("http://127.0.0.1:{port}"),
    };

    let path = req.uri().path().to_string();
    let method = req.method().clone();

    if method == Method::HEAD && path == "/upload" {
        return handle_head_upload(&state, &req, &admitted_refs);
    }
    if method == Method::PUT && path == "/upload" {
        return handle_put_upload(state, req, &request_base_url, &admitted_refs, upload_sem).await;
    }
    if method == Method::GET
        && let Some(raw_pk) = path.strip_prefix("/list/")
        && !raw_pk.contains('/')
    {
        return handle_list(&state, raw_pk, &request_base_url);
    }
    if method == Method::DELETE
        && let Some(sha256) = extract_sha256_segment(&path)
    {
        return handle_delete(&state, &req, sha256, &admitted_refs);
    }
    if (method == Method::GET || method == Method::HEAD)
        && let Some(sha256) = extract_sha256_segment(&path)
    {
        return handle_get_or_head(&state, &req, sha256, method == Method::HEAD).await;
    }

    error_response(StatusCode::NOT_FOUND, "not found")
}

fn handle_head_upload(
    state: &BlossomState,
    req: &Request<Incoming>,
    admitted_servers: &[&str],
) -> Response<BlossomBody> {
    let Some(x_sha256) = header_str(req, "x-sha-256").map(|s| s.trim().to_ascii_lowercase()) else {
        return error_response(StatusCode::BAD_REQUEST, "missing X-SHA-256 header");
    };
    if !is_valid_sha256_hex(&x_sha256) {
        return error_response(StatusCode::BAD_REQUEST, "invalid X-SHA-256 header");
    }

    let expected_len = match header_str(req, "x-content-length") {
        Some(raw) => match raw.trim().parse::<u64>() {
            Ok(0) => {
                return error_response(StatusCode::BAD_REQUEST, "X-Content-Length must be > 0");
            }
            Ok(n) if n > MAX_BLOSSOM_UPLOAD_BYTES => {
                return error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "X-Content-Length exceeds 100 MiB maximum",
                );
            }
            Ok(n) => Some(n),
            Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid X-Content-Length"),
        },
        None => None,
    };

    if header_str(req, "x-content-type").is_some_and(|t| t.trim().is_empty()) {
        return error_response(StatusCode::BAD_REQUEST, "X-Content-Type must not be empty");
    }

    let auth = match verify_blossom_auth(
        header_str(req, AUTHORIZATION.as_str()),
        AuthAction::Upload,
        now_secs(),
        &state.config.allowed_authors,
        Some(x_sha256.as_str()),
        admitted_servers,
    ) {
        Ok(a) => a,
        Err(err) => {
            state.record_decision(
                "head_upload",
                Some(x_sha256),
                None,
                false,
                err.reason().to_string(),
            );
            let status = match err {
                crate::auth::AuthError::MissingHeader | crate::auth::AuthError::InvalidScheme => {
                    StatusCode::UNAUTHORIZED
                }
                crate::auth::AuthError::AuthorNotAllowed
                | crate::auth::AuthError::ServerMismatch => StatusCode::FORBIDDEN,
                _ => StatusCode::BAD_REQUEST,
            };
            return error_response(status, err.reason());
        }
    };

    if let Some(len) = expected_len
        && let Err(err) =
            state
                .store
                .check_quota(Some(x_sha256.as_str()), len, state.config.max_bytes)
    {
        let reason = err.to_string();
        let status = match err {
            StorageError::QuotaExceeded { .. } => StatusCode::INSUFFICIENT_STORAGE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        state.record_decision(
            "head_upload",
            Some(x_sha256),
            Some(auth.pubkey_hex),
            false,
            reason.clone(),
        );
        return error_response(status, &reason);
    }

    let mut resp = Response::new(BlossomBody::empty());
    *resp.status_mut() = StatusCode::OK;
    add_cors_headers(&mut resp);
    resp
}

async fn handle_put_upload(
    state: Arc<BlossomState>,
    req: Request<Incoming>,
    base_url: &str,
    admitted_servers: &[&str],
    upload_sem: Arc<Semaphore>,
) -> Response<BlossomBody> {
    let header_sha = header_str(&req, "x-sha-256").map(|s| s.trim().to_ascii_lowercase());
    if let Some(ref hash) = header_sha
        && !is_valid_sha256_hex(hash)
    {
        return error_response(StatusCode::BAD_REQUEST, "invalid X-SHA-256 header");
    }

    let mime_type = header_str(&req, CONTENT_TYPE.as_str())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let auth = match verify_blossom_auth(
        header_str(&req, AUTHORIZATION.as_str()),
        AuthAction::Upload,
        now_secs(),
        &state.config.allowed_authors,
        header_sha.as_deref(),
        admitted_servers,
    ) {
        Ok(a) => a,
        Err(err) => {
            state.record_decision("upload", header_sha, None, false, err.reason().to_string());
            let status = match err {
                crate::auth::AuthError::MissingHeader | crate::auth::AuthError::InvalidScheme => {
                    StatusCode::UNAUTHORIZED
                }
                crate::auth::AuthError::AuthorNotAllowed
                | crate::auth::AuthError::ServerMismatch => StatusCode::FORBIDDEN,
                _ => StatusCode::BAD_REQUEST,
            };
            return error_response(status, err.reason());
        }
    };

    if let Some(len_hdr) = header_str(&req, CONTENT_LENGTH.as_str())
        && let Ok(declared_len) = len_hdr.trim().parse::<u64>()
    {
        if declared_len > MAX_BLOSSOM_UPLOAD_BYTES {
            let msg = "upload exceeds 100 MiB maximum";
            state.record_decision(
                "upload",
                header_sha,
                Some(auth.pubkey_hex),
                false,
                msg.to_string(),
            );
            return error_response(StatusCode::PAYLOAD_TOO_LARGE, msg);
        }
        if let Err(err) =
            state
                .store
                .check_quota(header_sha.as_deref(), declared_len, state.config.max_bytes)
        {
            let reason = err.to_string();
            let status = match err {
                StorageError::QuotaExceeded { .. } => StatusCode::INSUFFICIENT_STORAGE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            state.record_decision(
                "upload",
                header_sha,
                Some(auth.pubkey_hex),
                false,
                reason.clone(),
            );
            return error_response(status, &reason);
        }
    }

    let Ok(_upload_permit) = upload_sem.try_acquire_owned() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "too many concurrent uploads",
        );
    };

    let temp_path = state.store.temp_path(state.next_nonce());
    let mut temp_file = match tokio::fs::File::create(&temp_path).await {
        Ok(f) => f,
        Err(err) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to stage upload: {err}"),
            );
        }
    };

    let mut body = req.into_body();
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;

    let stream_outcome = tokio::time::timeout(UPLOAD_BODY_TIMEOUT, async {
        loop {
            let next = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;
            match next {
                Some(Ok(frame)) => {
                    if let Some(chunk) = frame.data_ref() {
                        written = written.saturating_add(chunk.len() as u64);
                        if written > MAX_BLOSSOM_UPLOAD_BYTES {
                            return Err((
                                StatusCode::PAYLOAD_TOO_LARGE,
                                "upload exceeds 100 MiB maximum".to_string(),
                                true,
                            ));
                        }
                        hasher.update(chunk);
                        if let Err(err) = temp_file.write_all(chunk).await {
                            return Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("failed writing upload: {err}"),
                                false,
                            ));
                        }
                    }
                }
                Some(Err(err)) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("stream interrupted: {err}"),
                        false,
                    ));
                }
                None => return Ok(()),
            }
        }
    })
    .await;

    match stream_outcome {
        Ok(Ok(())) => {}
        Ok(Err((status, msg, record))) => {
            drop(temp_file);
            let _ = tokio::fs::remove_file(&temp_path).await;
            if record {
                state.record_decision("upload", None, Some(auth.pubkey_hex), false, msg.clone());
            }
            return error_response(status, &msg);
        }
        Err(_) => {
            drop(temp_file);
            let _ = tokio::fs::remove_file(&temp_path).await;
            let msg = "upload stream timed out";
            state.record_decision(
                "upload",
                header_sha,
                Some(auth.pubkey_hex),
                false,
                msg.to_string(),
            );
            return error_response(StatusCode::REQUEST_TIMEOUT, msg);
        }
    }

    if let Err(err) = temp_file.flush().await {
        drop(temp_file);
        let _ = tokio::fs::remove_file(&temp_path).await;
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed flushing upload: {err}"),
        );
    }
    drop(temp_file);

    if written == 0 {
        let _ = tokio::fs::remove_file(&temp_path).await;
        let msg = "empty upload is not allowed";
        state.record_decision(
            "upload",
            None,
            Some(auth.pubkey_hex),
            false,
            msg.to_string(),
        );
        return error_response(StatusCode::BAD_REQUEST, msg);
    }

    let computed_sha = hex::encode(hasher.finalize());
    if let Some(ref hdr_sha) = header_sha
        && hdr_sha != &computed_sha
    {
        let _ = tokio::fs::remove_file(&temp_path).await;
        let msg = "X-SHA-256 does not match uploaded bytes";
        state.record_decision(
            "upload",
            Some(computed_sha),
            Some(auth.pubkey_hex),
            false,
            msg.to_string(),
        );
        return error_response(StatusCode::BAD_REQUEST, msg);
    }
    if !auth.x_hashes.iter().any(|h| h == &computed_sha) {
        let _ = tokio::fs::remove_file(&temp_path).await;
        let msg = "Authorization event x tag does not match uploaded sha256";
        state.record_decision(
            "upload",
            Some(computed_sha),
            Some(auth.pubkey_hex),
            false,
            msg.to_string(),
        );
        return error_response(StatusCode::FORBIDDEN, msg);
    }

    match state.store.commit_blob(
        &temp_path,
        &computed_sha,
        &mime_type,
        written,
        &auth.pubkey_hex,
        state.config.max_bytes,
    ) {
        Ok(record) => {
            state.record_decision(
                "upload",
                Some(computed_sha),
                Some(auth.pubkey_hex),
                true,
                "admitted".to_string(),
            );
            json_response(StatusCode::OK, blob_descriptor_json(&record, base_url))
        }
        Err(err) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            let reason = err.to_string();
            let status = match err {
                StorageError::QuotaExceeded { .. } => StatusCode::INSUFFICIENT_STORAGE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            state.record_decision(
                "upload",
                Some(computed_sha),
                Some(auth.pubkey_hex),
                false,
                reason.clone(),
            );
            error_response(status, &reason)
        }
    }
}

async fn handle_get_or_head(
    state: &BlossomState,
    req: &Request<Incoming>,
    sha256: &str,
    head_only: bool,
) -> Response<BlossomBody> {
    let record = match state.store.get_blob(sha256) {
        Ok(Some(rec)) => rec,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "blob not found"),
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    let path = state.store.blob_path(sha256);
    if !path.is_file() {
        return error_response(StatusCode::NOT_FOUND, "blob file missing on disk");
    }

    let range_hdr = header_str(req, RANGE.as_str());
    let (status, start, len, content_range) = if let Some(raw_range) = range_hdr {
        match parse_byte_range(raw_range, record.size) {
            Ok((start, end)) => (
                StatusCode::PARTIAL_CONTENT,
                start,
                end - start + 1,
                Some(format!("bytes {start}-{end}/{}", record.size)),
            ),
            Err(()) => {
                let mut resp = error_response(StatusCode::RANGE_NOT_SATISFIABLE, "invalid range");
                if let Ok(val) = HeaderValue::from_str(&format!("bytes */{}", record.size)) {
                    resp.headers_mut().insert(CONTENT_RANGE, val);
                }
                return resp;
            }
        }
    } else {
        (StatusCode::OK, 0, record.size, None)
    };

    let body = if head_only {
        BlossomBody::empty()
    } else {
        let mut file = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(err) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("failed to open blob: {err}"),
                );
            }
        };
        if start > 0
            && let Err(err) = file.seek(std::io::SeekFrom::Start(start)).await
        {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to seek blob: {err}"),
            );
        }
        BlossomBody::File {
            file,
            remaining: len,
        }
    };

    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    let headers = resp.headers_mut();
    let passive = is_passive_media_mime(&record.mime_type);
    if passive && let Ok(ct) = HeaderValue::from_str(&record.mime_type) {
        headers.insert(CONTENT_TYPE, ct);
    } else {
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        headers.insert(CONTENT_DISPOSITION, HeaderValue::from_static("attachment"));
    }
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Ok(cl) = HeaderValue::from_str(&len.to_string()) {
        headers.insert(CONTENT_LENGTH, cl);
    }
    headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some(cr) = content_range
        && let Ok(val) = HeaderValue::from_str(&cr)
    {
        headers.insert(CONTENT_RANGE, val);
    }
    add_cors_headers(&mut resp);
    resp
}

fn handle_delete(
    state: &BlossomState,
    req: &Request<Incoming>,
    sha256: &str,
    admitted_servers: &[&str],
) -> Response<BlossomBody> {
    let auth = match verify_blossom_auth(
        header_str(req, AUTHORIZATION.as_str()),
        AuthAction::Delete,
        now_secs(),
        &state.config.allowed_authors,
        Some(sha256),
        admitted_servers,
    ) {
        Ok(a) => a,
        Err(err) => {
            state.record_decision(
                "delete",
                Some(sha256.to_string()),
                None,
                false,
                err.reason().to_string(),
            );
            let status = match err {
                crate::auth::AuthError::MissingHeader | crate::auth::AuthError::InvalidScheme => {
                    StatusCode::UNAUTHORIZED
                }
                crate::auth::AuthError::AuthorNotAllowed
                | crate::auth::AuthError::ServerMismatch => StatusCode::FORBIDDEN,
                _ => StatusCode::BAD_REQUEST,
            };
            return error_response(status, err.reason());
        }
    };

    match state.store.delete_owner(sha256, &auth.pubkey_hex) {
        Ok(DeleteOutcome::NotFound) => {
            state.record_decision(
                "delete",
                Some(sha256.to_string()),
                Some(auth.pubkey_hex),
                false,
                "blob not found".to_string(),
            );
            error_response(StatusCode::NOT_FOUND, "blob not found")
        }
        Ok(DeleteOutcome::NotOwner) => {
            state.record_decision(
                "delete",
                Some(sha256.to_string()),
                Some(auth.pubkey_hex),
                false,
                "pubkey is not an owner of this blob".to_string(),
            );
            error_response(StatusCode::FORBIDDEN, "pubkey does not own this blob")
        }
        Ok(DeleteOutcome::RefRemoved { remaining_owners }) => {
            state.record_decision(
                "delete",
                Some(sha256.to_string()),
                Some(auth.pubkey_hex),
                true,
                format!("owner_ref_removed (remaining_owners={remaining_owners})"),
            );
            json_response(
                StatusCode::OK,
                json!({ "deleted": sha256, "blob_unlinked": false, "remaining_owners": remaining_owners }),
            )
        }
        Ok(DeleteOutcome::BlobDeleted) => {
            state.record_decision(
                "delete",
                Some(sha256.to_string()),
                Some(auth.pubkey_hex),
                true,
                "blob_deleted".to_string(),
            );
            json_response(
                StatusCode::OK,
                json!({ "deleted": sha256, "blob_unlinked": true, "remaining_owners": 0 }),
            )
        }
        Err(err) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

fn handle_list(state: &BlossomState, raw_pubkey: &str, base_url: &str) -> Response<BlossomBody> {
    let Some(pk) = parse_pubkey(raw_pubkey) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid pubkey in /list/<pubkey>");
    };
    let pk_hex = hex::encode(pk);
    match state.store.list_by_owner(Some(&pk_hex), 200) {
        Ok(blobs) => {
            let list: Vec<_> = blobs
                .iter()
                .map(|b| blob_descriptor_json(b, base_url))
                .collect();
            json_response(StatusCode::OK, serde_json::Value::Array(list))
        }
        Err(err) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

struct BlossomFront {
    state: Arc<BlossomState>,
    port: u16,
    upload_sem: Arc<Semaphore>,
}

impl Service<Request<Incoming>> for BlossomFront {
    type Response = Response<BlossomBody>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let state = Arc::clone(&self.state);
        let port = self.port;
        let upload_sem = Arc::clone(&self.upload_sem);
        Box::pin(async move { Ok(handle_request(state, port, upload_sem, req).await) })
    }
}

pub async fn bind(port: u16) -> std::io::Result<(TcpListener, SocketAddr)> {
    let listener =
        TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

fn is_transient_accept_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::TimedOut
    ) || matches!(error.raw_os_error(), Some(23 | 24))
}

pub async fn serve(listener: TcpListener, state: Arc<BlossomState>, port: u16) {
    let conn_sem = Arc::new(Semaphore::new(MAX_HTTP_CONNECTIONS));
    let upload_sem = Arc::new(Semaphore::new(MAX_CONCURRENT_UPLOADS));
    loop {
        let (socket, remote) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) if is_transient_accept_error(&error) => {
                tracing::warn!(%error, "transient accept error on blossom listener; backing off");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
            Err(error) => {
                tracing::error!(%error, "blossom listener stopped accepting connections");
                return;
            }
        };
        let Ok(permit) = Arc::clone(&conn_sem).try_acquire_owned() else {
            drop(socket);
            continue;
        };
        let front = BlossomFront {
            state: Arc::clone(&state),
            port,
            upload_sem: Arc::clone(&upload_sem),
        };
        tokio::spawn(async move {
            let _permit = permit;
            let conn = http1::Builder::new()
                .keep_alive(false)
                .serve_connection(TokioIo::new(socket), front);
            if let Ok(Err(error)) = tokio::time::timeout(HTTP_CONNECTION_TIMEOUT, conn).await {
                tracing::debug!(%remote, %error, "blossom HTTP connection ended with an error");
            }
        });
    }
}
