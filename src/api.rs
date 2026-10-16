use crate::{
    ApiError, ApiResult, Shared, bad, profiles,
    store::{Account, ApiKey, Group, Settings, Usage, digest, id, now, token},
    unauthorized,
};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn is_https(headers: &HeaderMap) -> bool {
    // Forwarded HTTPS is accepted only when explicitly enabled behind a trusted proxy.
    std::env::var("TRUST_PROXY").as_deref() == Ok("1")
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            == Some("https")
}
fn cookie(headers: &HeaderMap) -> &str {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .find_map(|part| part.trim().strip_prefix("zako_session="))
        .unwrap_or("")
}
fn entry(state: &Shared, headers: &HeaderMap) -> Result<(), ApiError> {
    let cfg = state.store.config()?;
    let requested = headers
        .get("x-console-path")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("/");
    if requested != format!("/{}", cfg.settings.security_path) {
        return Err(ApiError(StatusCode::NOT_FOUND, "Not found".into()));
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let origin = reqwest::Url::parse(origin.to_str().unwrap_or(""))
            .map_err(|_| bad("Invalid origin"))?;
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        let authority = origin[url::Position::BeforeHost..url::Position::AfterPort].to_owned();
        if authority != host {
            return Err(ApiError(StatusCode::FORBIDDEN, "Origin mismatch".into()));
        }
    }
    Ok(())
}
pub fn auth(state: &Shared, headers: &HeaderMap) -> Result<(), ApiError> {
    entry(state, headers)?;
    if !state.store.authenticated(cookie(headers)) {
        return Err(unauthorized());
    }
    Ok(())
}
fn session_response(
    state: &Shared,
    headers: &HeaderMap,
    username: &str,
    hash: &str,
) -> Result<Response, ApiError> {
    let secret = token("");
    if !state
        .store
        .session_for_credentials(&secret, username, hash)?
    {
        return Err(unauthorized());
    }
    let secure = if is_https(headers) { "; Secure" } else { "" };
    Ok((
        [(
            header::SET_COOKIE,
            format!(
                "zako_session={secret}; Path=/; HttpOnly; SameSite=Strict; Max-Age=86400{secure}"
            ),
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}
fn password_hash(password: &str) -> Result<String, ApiError> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|h| h.to_string())
        .map_err(|_| bad("Password hashing failed"))
}
fn verify(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).ok().is_some_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}
fn valid_password(password: &str) -> Result<(), ApiError> {
    if password.len() > 1024 || !(12..=256).contains(&password.chars().count()) {
        return Err(bad("Password must contain 12 to 256 characters"));
    }
    Ok(())
}
fn name(value: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() || value.chars().count() > 100 {
        return Err(bad("Name must contain 1 to 100 characters"));
    }
    Ok(())
}

pub async fn bootstrap(State(s): State<Shared>, h: HeaderMap) -> ApiResult<Value> {
    entry(&s, &h)?;
    let c = s.store.config()?;
    let authenticated = s.store.authenticated(cookie(&h));
    Ok(Json(
        json!({"initialized":!c.password_hash.is_empty(),"authenticated":authenticated,"username":if authenticated {c.username} else {String::new()}}),
    ))
}
#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}
pub async fn setup(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(input): Json<Credentials>,
) -> Result<Response, ApiError> {
    entry(&s, &h)?;
    name(&input.username)?;
    valid_password(&input.password)?;
    if !s.store.config()?.password_hash.is_empty() {
        return Err(bad("Administrator is already initialized"));
    }
    let hash = tokio::task::spawn_blocking(move || password_hash(&input.password))
        .await
        .map_err(|_| bad("Password hashing failed"))??;
    let username = input.username.trim().to_owned();
    s.store.update(|c| {
        if !c.password_hash.is_empty() {
            return Err("Administrator is already initialized".into());
        }
        c.username = username.clone();
        c.password_hash = hash.clone();
        Ok(())
    })?;
    session_response(&s, &h, &username, &hash)
}
pub async fn login(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(input): Json<Credentials>,
) -> Result<Response, ApiError> {
    entry(&s, &h)?;
    // A global budget also prevents distributed username/IP rotation on single-admin installs.
    {
        let mut attempts = s.login_attempts.lock().unwrap();
        let a = attempts.entry("admin".into()).or_insert((now(), 0));
        if now() - a.0 > 60 {
            *a = (now(), 0);
        }
        if a.1 >= 10 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many attempts; wait one minute".into(),
            ));
        }
        a.1 += 1;
    }
    let c = s.store.config()?;
    let password = input.password;
    let hash = c.password_hash.clone();
    let valid = tokio::task::spawn_blocking(move || verify(&password, &hash))
        .await
        .unwrap_or(false);
    if input.username != c.username || !valid {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "Invalid credentials".into(),
        ));
    }
    s.login_attempts.lock().unwrap().remove("admin");
    session_response(&s, &h, &c.username, &c.password_hash)
}
pub async fn logout(State(s): State<Shared>, h: HeaderMap) -> Result<Response, ApiError> {
    auth(&s, &h)?;
    s.store.logout(cookie(&h));
    Ok((
        [(
            header::SET_COOKIE,
            "zako_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}
pub async fn groups(State(s): State<Shared>, h: HeaderMap) -> ApiResult<Value> {
    auth(&s, &h)?;
    Ok(Json(json!(s.store.config()?.groups)))
}
pub async fn save_group(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(g): Json<Group>,
) -> ApiResult<Value> {
    save_group_impl(&s, &h, None, g)
}
pub async fn save_group_id(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(g): Json<Group>,
) -> ApiResult<Value> {
    save_group_impl(&s, &h, Some(id), g)
}
fn save_group_impl(
    s: &Shared,
    h: &HeaderMap,
    path: Option<String>,
    mut g: Group,
) -> ApiResult<Value> {
    auth(s, h)?;
    name(&g.name)?;
    if g.instruction.len() > 256 * 1024 || g.models.len() > 200 || g.profile_ids.len() > 16 {
        return Err(bad("Group configuration is too large"));
    }
    if g.profile_ids.iter().any(|id| profiles::find(id).is_none()) {
        return Err(bad("Unknown instruction profile"));
    }
    let mut selected = std::collections::HashSet::new();
    g.profile_ids.retain(|id| selected.insert(id.clone()));
    g.id = path.clone().unwrap_or_else(id);
    g.name = g.name.trim().into();
    s.store.update(|c| {
        if let Some(p) = path {
            let existing = c
                .groups
                .iter_mut()
                .find(|x| x.id == p)
                .ok_or("Group not found")?;
            *existing = g.clone();
        } else {
            c.groups.push(g.clone());
        }
        Ok(())
    })?;
    Ok(Json(json!(g)))
}
pub async fn delete_group(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    s.store.update(|c| {
        if c.accounts.iter().any(|a| a.group_ids.contains(&id))
            || c.keys.iter().any(|k| k.group_id == id)
        {
            return Err("Group is in use by accounts or API keys".into());
        }
        c.groups.retain(|g| g.id != id);
        Ok(())
    })?;
    Ok(Json(json!({"ok":true})))
}
pub async fn accounts(State(s): State<Shared>, h: HeaderMap) -> ApiResult<Value> {
    auth(&s, &h)?;
    let c = s.store.config()?;
    let runtime = s.runtime.lock().unwrap();
    Ok(Json(Value::Array(
        c.accounts
            .iter()
            .map(|a| {
                let mut v = json!(a);
                v.as_object_mut().unwrap().remove("api_key");
                v["has_api_key"] = json!(!a.api_key.is_empty());
                v["runtime"] = json!(runtime.get(&a.id).cloned().unwrap_or_default());
                v
            })
            .collect(),
    )))
}
pub async fn save_account(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(a): Json<Account>,
) -> ApiResult<Value> {
    save_account_impl(&s, &h, None, a)
}
pub async fn save_account_id(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(a): Json<Account>,
) -> ApiResult<Value> {
    save_account_impl(&s, &h, Some(id), a)
}
fn save_account_impl(
    s: &Shared,
    h: &HeaderMap,
    path: Option<String>,
    mut a: Account,
) -> ApiResult<Value> {
    auth(s, h)?;
    name(&a.name)?;
    crate::proxy::upstream_url(&a.endpoint, "models")?;
    if a.protocols.is_empty()
        || a.protocols
            .iter()
            .any(|p| p != "responses" && p != "compact")
    {
        return Err(bad("Select responses or compact protocol"));
    }
    if a.api_key.len() > 4096 || a.models.len() > 500 {
        return Err(bad("Account configuration is too large"));
    }
    a.id = path.clone().unwrap_or_else(id);
    if !a.api_key.is_empty() {
        a.api_key = s.store.seal(&a.api_key)?;
    }
    s.store.update(|c| {
        if a.group_ids
            .iter()
            .any(|g| !c.groups.iter().any(|v| &v.id == g))
        {
            return Err("Unknown group".into());
        }
        if let Some(p) = path {
            let existing = c
                .accounts
                .iter_mut()
                .find(|x| x.id == p)
                .ok_or("Account not found")?;
            if a.api_key.is_empty() {
                a.api_key = existing.api_key.clone();
            }
            *existing = a.clone();
        } else {
            if a.api_key.is_empty() {
                return Err("Upstream API key is required".into());
            }
            c.accounts.push(a.clone());
        }
        Ok(())
    })?;
    let mut out = json!(a);
    out.as_object_mut().unwrap().remove("api_key");
    Ok(Json(out))
}
pub async fn delete_account(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    s.store.update(|c| {
        c.accounts.retain(|a| a.id != id);
        Ok(())
    })?;
    s.runtime.lock().unwrap().remove(&id);
    Ok(Json(json!({"ok":true})))
}
pub async fn keys(State(s): State<Shared>, h: HeaderMap) -> ApiResult<Value> {
    auth(&s, &h)?;
    let c = s.store.config()?;
    let logs = s.store.logs(0, now())?;
    Ok(Json(Value::Array(c.keys.iter().map(|k|{
        let mut u=Usage::default();for l in logs.iter().filter(|l|l.key_id==k.id){u.input_tokens+=l.usage.input_tokens;u.output_tokens+=l.usage.output_tokens;u.cached_tokens+=l.usage.cached_tokens;}
        json!({"id":k.id,"name":k.name,"group_id":k.group_id,"prefix":k.prefix,"enabled":k.enabled,"created_at":k.created_at,"usage":u})
    }).collect())))
}
#[derive(Deserialize)]
pub struct KeyInput {
    name: String,
    group_id: String,
    #[serde(default = "key_enabled")]
    enabled: bool,
}
fn key_enabled() -> bool {
    true
}
pub async fn create_key(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(input): Json<KeyInput>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    name(&input.name)?;
    let key = token("sk-zako-");
    let k = ApiKey {
        id: id(),
        name: input.name,
        group_id: input.group_id,
        hash: digest(&key),
        secret: s.store.seal(&key)?,
        prefix: format!("{}...{}", &key[..15], &key[key.len() - 4..]),
        enabled: true,
        created_at: now(),
    };
    s.store.update(|c| {
        if !c.groups.iter().any(|g| g.id == k.group_id) {
            return Err("Unknown group".into());
        }
        c.keys.push(k.clone());
        Ok(())
    })?;
    Ok(Json(json!({"id":k.id,"key":key})))
}
pub async fn update_key(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<KeyInput>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    name(&input.name)?;
    s.store.update(|c| {
        if !c.groups.iter().any(|g| g.id == input.group_id) {
            return Err("Unknown group".into());
        }
        let k = c
            .keys
            .iter_mut()
            .find(|k| k.id == id)
            .ok_or("Key not found")?;
        k.name = input.name;
        k.group_id = input.group_id;
        k.enabled = input.enabled;
        Ok(())
    })?;
    Ok(Json(json!({"ok":true})))
}
pub async fn delete_key(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    s.store.update(|c| {
        c.keys.retain(|k| k.id != id);
        Ok(())
    })?;
    Ok(Json(json!({"ok":true})))
}
pub async fn reveal_key(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    let c = s.store.config()?;
    let k = c
        .keys
        .iter()
        .find(|k| k.id == id)
        .ok_or_else(|| bad("Key not found"))?;
    Ok(Json(json!({"key":s.store.unseal(&k.secret)?})))
}
pub async fn settings(State(s): State<Shared>, h: HeaderMap) -> ApiResult<Value> {
    auth(&s, &h)?;
    Ok(Json(json!(s.store.config()?.settings)))
}
pub async fn save_settings(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(mut settings): Json<Settings>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    settings.security_path = settings.security_path.trim_matches('/').into();
    if !settings
        .security_path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || settings.security_path.len() > 100
    {
        return Err(bad(
            "Security entry accepts letters, numbers, hyphens and underscores",
        ));
    }
    if ["api", "v1", "health", "assets", "favicon.svg"].contains(&settings.security_path.as_str()) {
        return Err(bad("Security entry is reserved"));
    }
    if settings.max_retries > 10
        || settings.failure_threshold == 0
        || settings.failure_threshold > 100
        || settings.cooldown_seconds == 0
        || settings.cooldown_seconds > 86400
        || settings.request_timeout_seconds < 5
        || settings.request_timeout_seconds > 3600
        || settings.retention_days == 0
        || settings.retention_days > 3650
    {
        return Err(bad("Scheduler values are out of range"));
    }
    s.store.update(|c| {
        c.settings = settings.clone();
        Ok(())
    })?;
    Ok(Json(json!(settings)))
}
#[derive(Deserialize)]
pub struct PasswordInput {
    current_password: String,
    new_password: String,
    confirm_password: String,
}
pub async fn password(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(input): Json<PasswordInput>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    valid_password(&input.new_password)?;
    if input.new_password != input.confirm_password {
        return Err(bad("New password confirmation does not match"));
    }
    let old_hash = s.store.config()?.password_hash;
    let expected_hash = old_hash.clone();
    let old = input.current_password;
    let hash = tokio::task::spawn_blocking(move || {
        if !verify(&old, &old_hash) {
            return Err(bad("Current password is incorrect"));
        }
        password_hash(&input.new_password)
    })
    .await
    .map_err(|_| bad("Password hashing failed"))??;
    if !s.store.change_password(cookie(&h), &expected_hash, hash)? {
        return Err(unauthorized());
    }
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
pub struct DiscoverInput {
    endpoint: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    account_id: Option<String>,
}
pub async fn discover(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(input): Json<DiscoverInput>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    let url = crate::proxy::upstream_url(&input.endpoint, "models")?;
    let key = if input.api_key.is_empty() {
        let c = s.store.config()?;
        let a = c
            .accounts
            .iter()
            .find(|a| Some(&a.id) == input.account_id.as_ref())
            .ok_or_else(|| bad("API key is required"))?;
        s.store.unseal(&a.api_key)?
    } else {
        input.api_key
    };
    let mut r = s
        .client
        .get(url)
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| bad("Upstream connection failed"))?;
    if !r.status().is_success() {
        return Err(bad(&format!(
            "Upstream returned HTTP {}",
            r.status().as_u16()
        )));
    }
    const MAX_MODEL_RESPONSE_BYTES: usize = 1024 * 1024;
    if r.content_length()
        .is_some_and(|size| size > MAX_MODEL_RESPONSE_BYTES as u64)
    {
        return Err(bad("Upstream model response is too large"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = r
        .chunk()
        .await
        .map_err(|_| bad("Upstream connection failed"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_MODEL_RESPONSE_BYTES {
            return Err(bad("Upstream model response is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    let v: Value =
        serde_json::from_slice(&body).map_err(|_| bad("Upstream returned invalid JSON"))?;
    let mut models: Vec<String> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.dedup();
    Ok(Json(json!({"models":models})))
}
pub async fn profiles(State(s): State<Shared>, h: HeaderMap) -> ApiResult<Value> {
    auth(&s, &h)?;
    Ok(Json(json!(profiles::all())))
}
#[derive(Deserialize)]
pub struct Range {
    from: Option<i64>,
    to: Option<i64>,
}
pub async fn stats(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(range): Query<Range>,
) -> ApiResult<Value> {
    auth(&s, &h)?;
    let end = range.to.unwrap_or_else(now);
    let start = range.from.unwrap_or(end - 7 * 86400);
    if start > end {
        return Err(bad("Invalid date range"));
    }
    let logs = s.store.logs(start, end)?;
    let live = s.store.logs(now() - 60, now())?;
    let c = s.store.config()?;
    let mut input = 0u64;
    let mut output = 0u64;
    let mut cached = 0u64;
    let mut latency = 0u64;
    let mut successes = 0u64;
    let mut cyber = 0u64;
    let mut daily: BTreeMap<String, Value> = BTreeMap::new();
    for l in &logs {
        input += l.usage.input_tokens;
        output += l.usage.output_tokens;
        cached += l.usage.cached_tokens;
        latency += l.latency_ms;
        let ok = (200..300).contains(&l.status);
        successes += u64::from(ok);
        cyber += u64::from(l.cyber);
        let date = chrono::DateTime::from_timestamp(l.timestamp, 0)
            .unwrap_or_default()
            .format("%Y-%m-%d")
            .to_string();
        let d=daily.entry(date.clone()).or_insert(json!({"date":date,"input_tokens":0,"output_tokens":0,"cached_tokens":0,"total_requests":0,"success_count":0,"cyber_count":0}));
        for (key, n) in [
            ("input_tokens", l.usage.input_tokens),
            ("output_tokens", l.usage.output_tokens),
            ("cached_tokens", l.usage.cached_tokens),
            ("total_requests", 1),
            ("success_count", u64::from(ok)),
            ("cyber_count", u64::from(l.cyber)),
        ] {
            d[key] = json!(d[key].as_u64().unwrap_or(0) + n);
        }
    }
    let runtime = s.runtime.lock().unwrap();
    let cooling = c
        .accounts
        .iter()
        .filter(|a| runtime.get(&a.id).is_some_and(|r| r.cooldown_until > now()))
        .count();
    let n = logs.len() as f64;
    let healthy = c
        .accounts
        .iter()
        .filter(|a| a.enabled && !runtime.get(&a.id).is_some_and(|r| r.cooldown_until > now()))
        .count();
    let recent:Vec<Value>=logs.iter().take(100).map(|l|json!({"id":l.id,"created_at":l.timestamp,"model":l.model,"account_name":c.accounts.iter().find(|a|a.id==l.account_id).map(|a|&a.name),"key_name":c.keys.iter().find(|k|k.id==l.key_id).map(|k|&k.name),"status":l.status,"latency_ms":l.latency_ms,"input_tokens":l.usage.input_tokens,"output_tokens":l.usage.output_tokens,"cached_tokens":l.usage.cached_tokens,"cyber_count":u8::from(l.cyber),"attempts":l.attempts,"protocol":l.protocol})).collect();
    Ok(Json(
        json!({"summary":{"input_tokens":input,"output_tokens":output,"cached_tokens":cached,"uncached_tokens":input.saturating_sub(cached),"total_tokens":input+output,"cache_hit_rate":if input>0{cached as f64/input as f64*100.0}else{0.0},"rpm":live.len(),"tpm":live.iter().map(|l|l.usage.input_tokens+l.usage.output_tokens).sum::<u64>(),"total_requests":logs.len(),"success_rate":if n>0.0{successes as f64/n*100.0}else{0.0},"avg_latency_ms":if n>0.0{latency as f64/n}else{0.0},"cyber_count":cyber},"daily":daily.into_values().collect::<Vec<_>>(),"recent":recent,"accounts":{"total":c.accounts.len(),"healthy":healthy,"cooling":cooling},"keys":c.keys.len()}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_verification() {
        let hash = password_hash("a-long-secret-password").unwrap();
        assert!(verify("a-long-secret-password", &hash));
        assert!(!verify("wrong", &hash));
        assert!(valid_password("short").is_err());
        assert!(valid_password(&"\u{4e00}".repeat(11)).is_err());
        assert!(valid_password(&"\u{4e00}".repeat(256)).is_ok());
        assert!(valid_password(&"a".repeat(257)).is_err());
    }
}
