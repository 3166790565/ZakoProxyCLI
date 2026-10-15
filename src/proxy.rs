use crate::{
    Shared,
    store::{self, Account, ApiKey, Config, Group, RequestLog, Settings, Usage},
};
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashSet},
    time::{Duration, Instant},
};

const MAX_EVENT_BYTES: usize = 256 * 1024;
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;

/// Endpoint roots may include /v1, but must not contain credentials or fragments.
pub fn upstream_url(endpoint: &str, route: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(endpoint.trim()).map_err(|_| "Invalid upstream endpoint")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(
            "Endpoint must be an HTTP(S) URL without credentials, query, or fragment".into(),
        );
    }
    let root = url.path().trim_end_matches('/');
    let root = if root.ends_with("/v1") {
        root.to_owned()
    } else {
        format!("{root}/v1")
    };
    url.set_path(&format!(
        "{root}/{}",
        route.trim_start_matches('/').trim_start_matches("v1/")
    ));
    Ok(url)
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"message":message,"type":"proxy_error"}})),
    )
        .into_response()
}

fn authorize(
    state: &Shared,
    headers: &HeaderMap,
) -> Result<(Config, ApiKey, Group), (StatusCode, &'static str)> {
    let raw = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, value)| value)
        .filter(|v| !v.is_empty())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "A valid proxy API key is required",
        ))?;
    let config = state.store.config().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Configuration unavailable",
        )
    })?;
    let hash = store::digest(raw);
    let key = config
        .keys
        .iter()
        .find(|key| key.enabled && key.hash == hash)
        .cloned()
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "A valid proxy API key is required",
        ))?;
    let group = config
        .groups
        .iter()
        .find(|group| group.id == key.group_id)
        .cloned()
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "The API key has no available group",
        ))?;
    Ok((config, key, group))
}

pub async fn models(State(state): State<Shared>, headers: HeaderMap) -> Response {
    let (config, _, group) = match authorize(&state, &headers) {
        Ok(value) => value,
        Err((status, message)) => return error(status, message),
    };
    let models: BTreeSet<_> = config
        .accounts
        .iter()
        .filter(|account| account.enabled && account.group_ids.contains(&group.id))
        .flat_map(|account| account.models.iter().cloned())
        .collect();
    Json(json!({"object":"list", "data":models.into_iter().map(|id| json!({"id":id,"object":"model","owned_by":"upstream"})).collect::<Vec<_>>()})).into_response()
}

pub async fn responses(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    forward(state, headers, body, "responses").await
}

pub async fn compact(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    forward(state, headers, body, "compact").await
}

fn apply_instruction(body: &mut Value, group: &Group, model: &str) -> Result<(), &'static str> {
    if group.instruction.is_empty() || !group.models.iter().any(|item| item == model) {
        return Ok(());
    }
    let instructions = match body.get("instructions") {
        None => group.instruction.clone(),
        Some(Value::String(existing)) if existing.is_empty() => group.instruction.clone(),
        Some(Value::String(existing)) => format!("{}\n\n{existing}", group.instruction),
        Some(_) => return Err("instructions must be a string when a group instruction is applied"),
    };
    body["instructions"] = Value::String(instructions);
    Ok(())
}

struct Admission {
    state: Shared,
    account_id: String,
}
impl Drop for Admission {
    fn drop(&mut self) {
        if let Ok(mut runtime) = self.state.runtime.lock()
            && let Some(account) = runtime.get_mut(&self.account_id)
        {
            account.active = account.active.saturating_sub(1);
        }
    }
}

fn admit(state: &Shared, account: &Account) -> Option<Admission> {
    let mut runtime = state.runtime.lock().ok()?;
    let runtime = runtime.entry(account.id.clone()).or_default();
    let now = store::now();
    if runtime.cooldown_until > now {
        return None;
    }
    if runtime.cooldown_until != 0 {
        runtime.cooldown_until = 0;
        runtime.failures = 0;
    }
    if runtime.window != now / 60 {
        runtime.window = now / 60;
        runtime.requests = 0;
    }
    if (account.concurrency > 0 && runtime.active >= account.concurrency)
        || (account.rpm > 0 && runtime.requests >= account.rpm)
    {
        return None;
    }
    runtime.active = runtime.active.saturating_add(1);
    runtime.requests = runtime.requests.saturating_add(1);
    Some(Admission {
        state: state.clone(),
        account_id: account.id.clone(),
    })
}

fn health(state: &Shared, account_id: &str, settings: &Settings, success: bool) {
    if let Ok(mut runtime) = state.runtime.lock() {
        let runtime = runtime.entry(account_id.to_owned()).or_default();
        if success {
            runtime.failures = 0;
            runtime.cooldown_until = 0;
        } else {
            runtime.failures = runtime.failures.saturating_add(1);
            if runtime.failures >= settings.failure_threshold.max(1) {
                runtime.cooldown_until = store::now() + i64::from(settings.cooldown_seconds);
            }
        }
    }
}

struct LogGuard {
    state: Shared,
    entry: RequestLog,
    started: Instant,
    retention: u32,
}
impl LogGuard {
    fn new(state: &Shared, key: &ApiKey, model: &str, protocol: &str, retention: u32) -> Self {
        Self {
            state: state.clone(),
            started: Instant::now(),
            retention,
            entry: RequestLog {
                id: store::id(),
                timestamp: store::now(),
                key_id: key.id.clone(),
                model: model.into(),
                protocol: protocol.into(),
                status: 499,
                ..Default::default()
            },
        }
    }
}
impl Drop for LogGuard {
    fn drop(&mut self) {
        self.entry.latency_ms = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        if let Err(error) = self.state.store.log(&self.entry, self.retention) {
            tracing::error!(request_id = %self.entry.id, %error, "Could not record proxy request");
        }
    }
}

async fn forward(state: Shared, headers: HeaderMap, mut body: Value, protocol: &str) -> Response {
    let (config, key, group) = match authorize(&state, &headers) {
        Ok(value) => value,
        Err((status, message)) => return error(status, message),
    };
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut log = LogGuard::new(
        &state,
        &key,
        &model,
        protocol,
        config.settings.retention_days,
    );
    if !body.is_object() || model.is_empty() {
        log.entry.status = 400;
        return error(
            StatusCode::BAD_REQUEST,
            "A JSON object with a nonempty model is required",
        );
    }
    if let Err(message) = apply_instruction(&mut body, &group, &model) {
        log.entry.status = 400;
        return error(StatusCode::BAD_REQUEST, message);
    }
    let mut accounts: Vec<_> = config
        .accounts
        .iter()
        .filter(|account| {
            account.enabled
                && account.group_ids.contains(&group.id)
                && account.protocols.iter().any(|item| item == protocol)
                && account.models.contains(&model)
        })
        .cloned()
        .collect();
    accounts.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
    if accounts.is_empty() {
        log.entry.status = 503;
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "No account supports this group, model, and protocol",
        );
    }
    let mut tried = HashSet::new();
    let mut reserved = None;
    let attempts = config.settings.max_retries.saturating_add(1);
    for _ in 0..attempts {
        let selected = reserved.take().or_else(|| {
            accounts
                .iter()
                .filter(|account| !tried.contains(&account.id))
                .find_map(|account| admit(&state, account).map(|admission| (account, admission)))
        });
        let Some((account, admission)) = selected else {
            log.entry.status = if log.entry.attempts == 0 { 429 } else { 502 };
            let mut response = error(
                StatusCode::from_u16(log.entry.status).unwrap(),
                "No upstream account is currently available",
            );
            response
                .headers_mut()
                .insert("retry-after", HeaderValue::from_static("1"));
            return response;
        };
        tried.insert(account.id.clone());
        log.entry.attempts += 1;
        log.entry.account_id = account.id.clone();
        log.entry.status = 499;
        let route = if protocol == "compact" {
            "responses/compact"
        } else {
            "responses"
        };
        let url = match upstream_url(&account.endpoint, route) {
            Ok(url) => url,
            Err(_) => {
                log.entry.status = 502;
                health(&state, &account.id, &config.settings, false);
                continue;
            }
        };
        let secret = match state.store.unseal(&account.api_key) {
            Ok(secret) => secret,
            Err(_) => {
                log.entry.status = 502;
                health(&state, &account.id, &config.settings, false);
                continue;
            }
        };
        let mut request = state
            .client
            .post(url)
            .bearer_auth(&secret)
            .json(&body)
            .timeout(Duration::from_secs(u64::from(
                config.settings.request_timeout_seconds.max(1),
            )));
        for name in [
            "accept",
            "openai-beta",
            "idempotency-key",
            "x-client-request-id",
        ] {
            if let Some(value) = headers.get(name) {
                request = request.header(name, value);
            }
        }
        let upstream = match request.send().await {
            Ok(response) => response,
            Err(_) => {
                log.entry.status = 502;
                health(&state, &account.id, &config.settings, false);
                continue;
            }
        };
        let status = upstream.status();
        let retryable = matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::REQUEST_TIMEOUT
                | StatusCode::TOO_MANY_REQUESTS
        ) || status.is_server_error();
        if retryable {
            health(&state, &account.id, &config.settings, false);
            if log.entry.attempts < attempts {
                reserved = accounts
                    .iter()
                    .filter(|item| !tried.contains(&item.id))
                    .find_map(|item| admit(&state, item).map(|admission| (item, admission)));
                if reserved.is_some() {
                    continue;
                }
            }
        }
        let mut response_headers = HeaderMap::new();
        for name in ["content-type", "x-request-id", "retry-after"] {
            if let Some(value) = upstream.headers().get(name) {
                // An upstream must not reflect its credential through response headers.
                if secret.is_empty()
                    || !value
                        .as_bytes()
                        .windows(secret.len())
                        .any(|part| part == secret.as_bytes())
                {
                    response_headers.insert(name, value.clone());
                }
            }
        }
        if !response_headers.contains_key("x-request-id")
            && let Ok(request_id) = HeaderValue::from_str(&log.entry.id)
        {
            response_headers.insert("x-request-id", request_id);
        }
        let sse = upstream
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().starts_with("text/event-stream"));
        let settings = config.settings.clone();
        let account_id = account.id.clone();
        let stream_state = state.clone();
        log.entry.status = 499;
        let output = async_stream::stream! {
            let _admission = admission;
            let mut log = log;
            let mut observer = Observer::new(sse);
            let mut redactor = SecretFilter::new(secret);
            let mut stream = upstream.bytes_stream();
            let mut transport_failed = false;
            let mut health_recorded = false;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(chunk) => {
                        observer.feed(&chunk);
                        log.entry.usage = observer.usage.clone();
                        log.entry.cyber = observer.cyber;
                        if status.is_success() && (observer.completed || observer.failed) && !health_recorded {
                            log.entry.status = if observer.failed { 502 } else { status.as_u16() };
                            health(&stream_state, &account_id, &settings, !observer.failed);
                            health_recorded = true;
                        }
                        let chunk = redactor.feed(&chunk);
                        if !chunk.is_empty() { yield Ok::<Bytes, std::io::Error>(chunk); }
                    }
                    Err(_) => { transport_failed = true; break; }
                }
            }
            observer.finish();
            log.entry.usage = observer.usage;
            log.entry.cyber = observer.cyber;
            let transport_failed = transport_failed && !(sse && (observer.completed || observer.failed));
            let incomplete = sse && status.is_success() && !observer.completed && !observer.failed;
            log.entry.status = if transport_failed || incomplete || (status.is_success() && observer.failed) { 502 } else { status.as_u16() };
            if status.is_success() && !health_recorded {
                health(&stream_state, &account_id, &settings, log.entry.status < 400);
            }
            let tail = redactor.finish();
            if !tail.is_empty() { yield Ok(tail); }
            if transport_failed || incomplete {
                yield Err(std::io::Error::other("Upstream response ended before completion"));
            }
        };
        let mut response = Response::new(Body::from_stream(output));
        *response.status_mut() = status;
        *response.headers_mut() = response_headers;
        return response;
    }
    log.entry.status = 502;
    error(StatusCode::BAD_GATEWAY, "Upstream request failed")
}

// Keep only one bounded event and a small marker window, never request/response content in logs.
struct Observer {
    sse: bool,
    line: Vec<u8>,
    event_name: String,
    data: Vec<u8>,
    oversized: bool,
    discard_line: bool,
    json: Vec<u8>,
    json_oversized: bool,
    marker: Vec<u8>,
    cyber: bool,
    usage: Usage,
    completed: bool,
    failed: bool,
}
impl Observer {
    fn new(sse: bool) -> Self {
        Self {
            sse,
            line: Vec::new(),
            event_name: String::new(),
            data: Vec::new(),
            oversized: false,
            discard_line: false,
            json: Vec::new(),
            json_oversized: false,
            marker: Vec::new(),
            cyber: false,
            usage: Usage::default(),
            completed: false,
            failed: false,
        }
    }
    fn feed(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.marker.push(byte.to_ascii_lowercase());
            if self.marker.len() > 5 {
                self.marker.remove(0);
            }
            if self.marker == b"cyber" {
                self.cyber = true;
            }
        }
        if !self.sse {
            if !self.json_oversized && self.json.len().saturating_add(bytes.len()) <= MAX_JSON_BYTES
            {
                self.json.extend_from_slice(bytes);
            } else {
                self.json.clear();
                self.json_oversized = true;
            }
            return;
        }
        for byte in bytes {
            if *byte == b'\n' {
                if self.discard_line {
                    self.line.clear();
                    self.discard_line = false;
                } else {
                    self.line();
                }
            } else if !self.discard_line {
                if self.line.len() < MAX_EVENT_BYTES {
                    self.line.push(*byte);
                } else {
                    self.line.clear();
                    self.discard_line = true;
                    self.oversized = true;
                }
            }
        }
    }
    fn line(&mut self) {
        let line = std::mem::take(&mut self.line);
        let line = line.strip_suffix(b"\r").unwrap_or(&line);
        if line.is_empty() {
            match self.event_name.as_str() {
                "response.completed" => self.completed = true,
                "response.failed" | "response.incomplete" | "error" => self.failed = true,
                _ => {}
            }
            self.event_name.clear();
            if !self.oversized {
                let data = std::mem::take(&mut self.data);
                self.event(&data);
            }
            self.data.clear();
            self.oversized = false;
        } else if let Some(name) = line.strip_prefix(b"event:") {
            if name.len() < 128 {
                self.event_name = String::from_utf8_lossy(name).trim().to_owned();
            }
        } else if let Some(data) = line.strip_prefix(b"data:") {
            let data = data.strip_prefix(b" ").unwrap_or(data);
            if self.data.len().saturating_add(data.len()).saturating_add(1) <= MAX_EVENT_BYTES {
                self.data.extend_from_slice(data);
                self.data.push(b'\n');
            } else {
                self.data.clear();
                self.oversized = true;
            }
        }
    }
    fn event(&mut self, data: &[u8]) {
        if let Ok(value) = serde_json::from_slice::<Value>(data) {
            let usage = store::usage(&value);
            self.usage.input_tokens = self.usage.input_tokens.max(usage.input_tokens);
            self.usage.output_tokens = self.usage.output_tokens.max(usage.output_tokens);
            self.usage.cached_tokens = self.usage.cached_tokens.max(usage.cached_tokens);
            match value.get("type").and_then(Value::as_str) {
                Some("response.completed") => self.completed = true,
                Some("response.failed" | "response.incomplete" | "error") => self.failed = true,
                _ => {}
            }
            if value.get("error").is_some_and(|error| !error.is_null())
                || matches!(
                    value.get("status").and_then(Value::as_str),
                    Some("failed" | "incomplete")
                )
            {
                self.failed = true;
            }
        }
    }
    fn finish(&mut self) {
        if self.sse {
            if !self.line.is_empty() && !self.discard_line {
                self.line();
            }
            if !self.oversized {
                let data = std::mem::take(&mut self.data);
                self.event(&data);
            }
        } else if !self.json_oversized {
            let data = std::mem::take(&mut self.json);
            self.event(&data);
        }
    }
}

// A rolling suffix prevents credentials split between network chunks from leaking.
struct SecretFilter {
    secret: Vec<u8>,
    pending: Vec<u8>,
}
impl SecretFilter {
    fn new(secret: String) -> Self {
        Self {
            secret: secret.into_bytes(),
            pending: Vec::new(),
        }
    }
    fn feed(&mut self, bytes: &[u8]) -> Bytes {
        if self.secret.is_empty() {
            return Bytes::copy_from_slice(bytes);
        }
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::new();
        let mut index = 0;
        while index + self.secret.len() <= self.pending.len() {
            if self.pending[index..].starts_with(&self.secret) {
                output.extend_from_slice(b"[redacted]");
                index += self.secret.len();
            } else {
                output.push(self.pending[index]);
                index += 1;
            }
        }
        self.pending.drain(..index);
        Bytes::from(output)
    }
    fn finish(&mut self) -> Bytes {
        Bytes::from(std::mem::take(&mut self.pending))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_normalization_and_validation() {
        assert_eq!(
            upstream_url("https://example.com", "responses")
                .unwrap()
                .as_str(),
            "https://example.com/v1/responses"
        );
        assert_eq!(
            upstream_url("https://example.com/v1/", "responses/compact")
                .unwrap()
                .as_str(),
            "https://example.com/v1/responses/compact"
        );
        assert_eq!(
            upstream_url("http://localhost:1999/proxy", "/v1/models")
                .unwrap()
                .as_str(),
            "http://localhost:1999/proxy/v1/models"
        );
        for invalid in [
            "file:///etc/passwd",
            "https://user:pass@example.com",
            "https://example.com?token=x",
            "https://example.com/#x",
        ] {
            assert!(upstream_url(invalid, "models").is_err());
        }
    }
    #[test]
    fn instruction_changes_only_selected_model_and_preserves_fields() {
        let group = Group {
            models: vec!["selected".into()],
            instruction: "Configured instruction".into(),
            ..Default::default()
        };
        let original = json!({"model":"other","instructions":"Existing","input":[{"role":"user","content":"hello"}],"tools":[{"type":"function"}],"store":false});
        let mut body = original.clone();
        apply_instruction(&mut body, &group, "other").unwrap();
        assert_eq!(body, original);
        apply_instruction(&mut body, &group, "selected").unwrap();
        assert_eq!(body["instructions"], "Configured instruction\n\nExisting");
        assert_eq!(body["input"], original["input"]);
        assert_eq!(body["tools"], original["tools"]);
        assert_eq!(body["store"], original["store"]);
        assert!(apply_instruction(&mut json!({"instructions":[]}), &group, "selected").is_err());
    }
    #[test]
    fn sse_usage_survives_every_byte_boundary() {
        let bytes = b"event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"CYBER\"}\r\n\r\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":100,\"output_tokens\":8,\"input_tokens_details\":{\"cached_tokens\":60}}}}\n\n";
        for split in 0..=bytes.len() {
            let mut observer = Observer::new(true);
            observer.feed(&bytes[..split]);
            observer.feed(&bytes[split..]);
            observer.finish();
            assert_eq!(observer.usage.input_tokens, 100);
            assert_eq!(observer.usage.cached_tokens, 60);
            assert!(observer.completed);
            assert!(observer.cyber);
        }
        let mut observer = Observer::new(true);
        for byte in bytes {
            observer.feed(&[*byte]);
        }
        observer.finish();
        assert_eq!(observer.usage.output_tokens, 8);
    }
    #[test]
    fn oversized_sse_events_are_bounded_and_next_event_works() {
        let mut observer = Observer::new(true);
        observer.feed(b"data: ");
        observer.feed(&vec![b'x'; MAX_EVENT_BYTES + 50]);
        observer.feed(b"\n\ndata: {\"type\":\"response.completed\"}\n\n");
        assert!(observer.completed);
        assert!(observer.line.len() <= MAX_EVENT_BYTES);
        assert!(observer.data.len() <= MAX_EVENT_BYTES);
    }
    #[test]
    fn credentials_are_redacted_across_all_boundaries() {
        let bytes = b"before sk-private after sk-private";
        for split in 0..=bytes.len() {
            let mut filter = SecretFilter::new("sk-private".into());
            let output = [
                filter.feed(&bytes[..split]),
                filter.feed(&bytes[split..]),
                filter.finish(),
            ]
            .concat();
            assert_eq!(output, b"before [redacted] after [redacted]");
        }
    }
    #[test]
    fn admission_is_atomic_and_drop_releases_concurrency() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::new_app(dir.path(), dir.path().into()).unwrap();
        let account = Account {
            id: "a".into(),
            concurrency: 1,
            rpm: 2,
            ..Default::default()
        };
        let guard = admit(&state, &account).unwrap();
        assert!(admit(&state, &account).is_none());
        drop(guard);
        let guard = admit(&state, &account).unwrap();
        drop(guard);
        assert!(admit(&state, &account).is_none());
        assert_eq!(state.runtime.lock().unwrap()["a"].active, 0);
        health(
            &state,
            "a",
            &Settings {
                failure_threshold: 1,
                ..Default::default()
            },
            false,
        );
        assert!(state.runtime.lock().unwrap()["a"].cooldown_until > store::now());
    }

    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    async fn serve(router: axum::Router) -> (String, Server) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (endpoint, Server(task))
    }
    fn fixture(endpoints: &[String]) -> (tempfile::TempDir, Shared) {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::new_app(dir.path(), dir.path().into()).unwrap();
        let secret = state.store.seal("upstream-secret").unwrap();
        state
            .store
            .update(|config| {
                config.groups.push(Group {
                    id: "group".into(),
                    models: vec!["chosen".into()],
                    instruction: "Configured instruction".into(),
                    ..Default::default()
                });
                config.keys.push(ApiKey {
                    id: "key".into(),
                    group_id: "group".into(),
                    hash: store::digest("proxy-key"),
                    enabled: true,
                    ..Default::default()
                });
                config.settings.failure_threshold = 1;
                for (index, endpoint) in endpoints.iter().enumerate() {
                    config.accounts.push(Account {
                        id: format!("account-{index}"),
                        endpoint: endpoint.clone(),
                        api_key: secret.clone(),
                        group_ids: vec!["group".into()],
                        models: vec!["chosen".into()],
                        protocols: vec!["responses".into(), "compact".into()],
                        enabled: true,
                        priority: index as i32,
                        ..Default::default()
                    });
                }
                Ok(())
            })
            .unwrap();
        (dir, state)
    }
    async fn call(state: Shared, path: &str, body: Value) -> Response {
        use tower::ServiceExt;
        crate::app(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", "Bearer proxy-key")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn no_selected_models_never_routes_as_wildcard() {
        let (_dir, state) = fixture(&["http://127.0.0.1:1".into()]);
        state
            .store
            .update(|config| {
                config.accounts[0].models.clear();
                Ok(())
            })
            .unwrap();
        let response = call(
            state.clone(),
            "/v1/responses",
            json!({"model":"chosen","input":"hello"}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(state.runtime.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mock_upstream_failover_preserves_compact_payload_and_logs_usage() {
        use axum::{http::Uri, routing::post};
        let (first, _first_server) = serve(axum::Router::new().route(
            "/v1/responses/compact",
            post(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        ))
        .await;
        let received = std::sync::Arc::new(std::sync::Mutex::new(None));
        let observed = received.clone();
        let (second, _second_server) = serve(axum::Router::new().route("/v1/responses/compact", post(move |uri: Uri, headers: HeaderMap, Json(body): Json<Value>| {
            let observed = observed.clone(); async move {
                assert_eq!(headers["authorization"], "Bearer upstream-secret");
                *observed.lock().unwrap() = Some((uri.path().to_owned(), body));
                ([ ("x-request-id", "upstream-request") ], Json(json!({"id":"response","usage":{"input_tokens":20,"output_tokens":3,"input_tokens_details":{"cached_tokens":8}}})))
            }
        }))).await;
        let (_dir, state) = fixture(&[first, second]);
        let input = json!({"model":"chosen","instructions":"Existing","input":[{"role":"user","content":"hello"}],"tools":[{"type":"function","name":"tool"}],"metadata":{"custom":true},"store":false});
        let response = call(state.clone(), "/v1/responses/compact", input.clone()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-request-id"], "upstream-request");
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["id"],
            "response"
        );
        let captured = received.lock().unwrap().clone().unwrap();
        assert_eq!(captured.0, "/v1/responses/compact");
        assert_eq!(
            captured.1["instructions"],
            "Configured instruction\n\nExisting"
        );
        for field in ["input", "tools", "metadata", "store"] {
            assert_eq!(captured.1[field], input[field]);
        }
        let logs = state.store.logs(0, store::now() + 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, 200);
        assert_eq!(logs[0].attempts, 2);
        assert_eq!(logs[0].usage.cached_tokens, 8);
        let runtime = state.runtime.lock().unwrap();
        assert!(runtime["account-0"].cooldown_until > store::now());
        assert_eq!(runtime["account-0"].active, 0);
        assert_eq!(runtime["account-1"].active, 0);
    }
    #[tokio::test]
    async fn client_error_is_forwarded_without_retry_or_cooldown_and_secrets_are_redacted() {
        use axum::routing::post;
        let (endpoint, _server) = serve(axum::Router::new().route(
            "/v1/responses",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    [("content-type", "application/json"), ("retry-after", "7")],
                    r#"{"error":"upstream-secret invalid input"}"#,
                )
            }),
        ))
        .await;
        let (_dir, state) = fixture(&[endpoint]);
        let response = call(
            state.clone(),
            "/v1/responses",
            json!({"model":"chosen","input":"hello"}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["retry-after"], "7");
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("upstream-secret"));
        assert!(String::from_utf8_lossy(&bytes).contains("[redacted]"));
        let runtime = state.runtime.lock().unwrap();
        assert_eq!(runtime["account-0"].failures, 0);
        assert_eq!(runtime["account-0"].active, 0);
        let logs = state.store.logs(0, store::now() + 10).unwrap();
        assert_eq!(logs[0].status, 400);
        assert_eq!(logs[0].attempts, 1);
    }
    #[tokio::test]
    async fn dropping_unconsumed_stream_releases_admission_and_records_499() {
        use axum::routing::post;
        let (endpoint, _server) = serve(axum::Router::new().route("/v1/responses", post(|| async {
            let output = async_stream::stream! {
                yield Ok::<_, std::io::Error>(Bytes::from_static(b"data: {\"type\":\"response.created\"}\n\n"));
                tokio::time::sleep(Duration::from_secs(300)).await;
            };
            ([("content-type", "text/event-stream")], Body::from_stream(output))
        }))).await;
        let (_dir, state) = fixture(&[endpoint]);
        let response = call(
            state.clone(),
            "/v1/responses",
            json!({"model":"chosen","input":"hello","stream":true}),
        )
        .await;
        assert_eq!(state.runtime.lock().unwrap()["account-0"].active, 1);
        drop(response);
        assert_eq!(state.runtime.lock().unwrap()["account-0"].active, 0);
        let logs = state.store.logs(0, store::now() + 10).unwrap();
        assert_eq!(logs[0].status, 499);
    }

    #[tokio::test]
    async fn sse_success_bytes_and_usage_are_preserved() {
        use axum::routing::post;
        const SSE: &[u8] = b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"cyber\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":40,\"output_tokens\":6,\"input_tokens_details\":{\"cached_tokens\":30}}}}\n\ndata: [DONE]\n\n";
        let (endpoint, _server) = serve(axum::Router::new().route(
            "/v1/responses",
            post(|| async {
                let stream = futures_util::stream::iter(
                    SSE.chunks(11)
                        .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk))),
                );
                (
                    [("content-type", "text/event-stream")],
                    Body::from_stream(stream),
                )
            }),
        ))
        .await;
        let (_dir, state) = fixture(&[endpoint]);
        let response = call(
            state.clone(),
            "/v1/responses",
            json!({"model":"chosen","input":"hello","stream":true}),
        )
        .await;
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&body[..], SSE);
        let logs = state.store.logs(0, store::now() + 10).unwrap();
        assert_eq!(logs[0].status, 200);
        assert_eq!(logs[0].usage.cached_tokens, 30);
        assert!(logs[0].cyber);
        assert_eq!(state.runtime.lock().unwrap()["account-0"].active, 0);
    }

    #[tokio::test]
    async fn explicit_sse_failure_preserves_events_and_counts_failure() {
        use axum::routing::post;
        const SSE: &str = "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"usage\":{\"input_tokens\":7}}}\n\n";
        let (endpoint, _server) = serve(axum::Router::new().route(
            "/v1/responses",
            post(|| async { ([("content-type", "text/event-stream")], SSE) }),
        ))
        .await;
        let (_dir, state) = fixture(&[endpoint]);
        let response = call(
            state.clone(),
            "/v1/responses",
            json!({"model":"chosen","input":"hello","stream":true}),
        )
        .await;
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&body[..], SSE.as_bytes());
        let logs = state.store.logs(0, store::now() + 10).unwrap();
        assert_eq!(logs[0].status, 502);
        assert_eq!(logs[0].usage.input_tokens, 7);
        let runtime = state.runtime.lock().unwrap();
        assert_eq!(runtime["account-0"].active, 0);
        assert_eq!(runtime["account-0"].failures, 1);
    }

    #[tokio::test]
    async fn truncated_sse_reports_transport_failure_and_releases_slot() {
        use axum::routing::post;
        let (endpoint, _server) = serve(axum::Router::new().route(
            "/v1/responses",
            post(|| async {
                (
                    [("content-type", "text/event-stream")],
                    "data: {\"type\":\"response.created\"}\n\n",
                )
            }),
        ))
        .await;
        let (_dir, state) = fixture(&[endpoint]);
        let response = call(
            state.clone(),
            "/v1/responses",
            json!({"model":"chosen","input":"hello","stream":true}),
        )
        .await;
        assert!(
            axum::body::to_bytes(response.into_body(), 4096)
                .await
                .is_err()
        );
        assert_eq!(
            state.store.logs(0, store::now() + 10).unwrap()[0].status,
            502
        );
        assert_eq!(state.runtime.lock().unwrap()["account-0"].active, 0);
    }

    #[tokio::test]
    async fn unavailable_fallback_keeps_original_upstream_error() {
        use axum::routing::post;
        let (endpoint, _server) = serve(axum::Router::new().route(
            "/v1/responses",
            post(|| async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [("retry-after", "23")],
                    "original upstream failure",
                )
            }),
        ))
        .await;
        let (_dir, state) = fixture(&[endpoint.clone(), endpoint]);
        state
            .runtime
            .lock()
            .unwrap()
            .entry("account-1".into())
            .or_default()
            .cooldown_until = store::now() + 100;
        let response = call(
            state.clone(),
            "/v1/responses",
            json!({"model":"chosen","input":"hello"}),
        )
        .await;
        assert_eq!(response.status(), 503);
        assert_eq!(response.headers()["retry-after"], "23");
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&body[..], b"original upstream failure");
        let logs = state.store.logs(0, store::now() + 10).unwrap();
        assert_eq!(logs[0].status, 503);
        assert_eq!(logs[0].attempts, 1);
    }

    #[tokio::test]
    async fn client_disconnect_after_completed_event_keeps_success_status() {
        use axum::routing::post;
        let (endpoint, _server) = serve(axum::Router::new().route("/v1/responses", post(|| async {
            let output = async_stream::stream! {
                yield Ok::<_, std::io::Error>(Bytes::from_static(b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":2}}}\n\n"));
                tokio::time::sleep(Duration::from_secs(300)).await;
            };
            ([("content-type", "text/event-stream")], Body::from_stream(output))
        }))).await;
        let (_dir, state) = fixture(&[endpoint]);
        let response = call(
            state.clone(),
            "/v1/responses",
            json!({"model":"chosen","input":"hello","stream":true}),
        )
        .await;
        let mut stream = response.into_body().into_data_stream();
        assert!(stream.next().await.unwrap().is_ok());
        drop(stream);
        let logs = state.store.logs(0, store::now() + 10).unwrap();
        assert_eq!(logs[0].status, 200);
        assert_eq!(logs[0].usage.input_tokens, 12);
        assert_eq!(state.runtime.lock().unwrap()["account-0"].active, 0);
    }
}
