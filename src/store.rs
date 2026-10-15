use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};

pub type Result<T> = std::result::Result<T, String>;
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn token(prefix: &str) -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{prefix}{}", hex::encode(bytes))
}
pub fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Group {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub instruction: String,
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Account {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub group_ids: Vec<String>,
    #[serde(default)]
    pub rpm: u32,
    #[serde(default)]
    pub concurrency: u32,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default = "protocols")]
    pub protocols: Vec<String>,
}
fn enabled() -> bool {
    true
}
fn protocols() -> Vec<String> {
    vec!["responses".into(), "compact".into()]
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub group_id: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub created_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub security_path: String,
    pub max_retries: u32,
    pub failure_threshold: u32,
    pub cooldown_seconds: u32,
    pub request_timeout_seconds: u32,
    pub retention_days: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            security_path: String::new(),
            max_retries: 2,
            failure_threshold: 2,
            cooldown_seconds: 60,
            request_timeout_seconds: 120,
            retention_days: 90,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub username: String,
    pub password_hash: String,
    pub settings: Settings,
    pub groups: Vec<Group>,
    pub accounts: Vec<Account>,
    pub keys: Vec<ApiKey>,
}
#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct RequestLog {
    pub id: String,
    pub timestamp: i64,
    pub key_id: String,
    pub account_id: String,
    pub model: String,
    pub protocol: String,
    pub status: u16,
    pub latency_ms: u64,
    pub usage: Usage,
    pub cyber: bool,
    pub attempts: u32,
}
pub struct Store {
    db: Mutex<Connection>,
    cipher: Aes256Gcm,
}
fn read_config(db: &Connection) -> Result<Config> {
    db.query_row("SELECT value FROM config WHERE id=1", [], |row| {
        row.get::<_, String>(0)
    })
    .optional()
    .map_err(|e| e.to_string())?
    .map(|value| serde_json::from_str(&value).map_err(|e| e.to_string()))
    .transpose()
    .map(Option::unwrap_or_default)
}
fn write_config(db: &Connection, config: &Config) -> Result<()> {
    db.execute(
        "INSERT INTO config(id,value) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value",
        [serde_json::to_string(config).map_err(|e| e.to_string())?],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn insert_session(db: &Connection, raw: &str) -> Result<()> {
    db.execute("DELETE FROM sessions WHERE expires <= ?1", [now()])
        .map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO sessions VALUES(?1,?2)",
        params![digest(raw), now() + 86400],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = dir.join("master.key");
        let key = if path.exists() {
            std::fs::read(&path).map_err(|e| e.to_string())?
        } else {
            let mut key = vec![0u8; 32];
            rand::thread_rng().fill_bytes(&mut key);
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options
                .open(&path)
                .and_then(|mut f| f.write_all(&key))
                .map_err(|e| e.to_string())?;
            key
        };
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|_| "Invalid master.key".to_owned())?;
        let db = Connection::open(dir.join("zako.db")).map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS config (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS sessions (hash TEXT PRIMARY KEY, expires INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS requests (id TEXT PRIMARY KEY, timestamp INTEGER NOT NULL, value TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS requests_time ON requests(timestamp);") .map_err(|e| e.to_string())?;
        Ok(Self {
            db: Mutex::new(db),
            cipher,
        })
    }
    pub fn seal(&self, plain: &str) -> Result<String> {
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let data = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), plain.as_bytes())
            .map_err(|_| "Encryption failed")?;
        Ok(STANDARD.encode([nonce.to_vec(), data].concat()))
    }
    pub fn unseal(&self, encrypted: &str) -> Result<String> {
        let bytes = STANDARD
            .decode(encrypted)
            .map_err(|_| "Invalid encrypted secret")?;
        if bytes.len() < 28 {
            return Err("Invalid encrypted secret".into());
        }
        let plain = self
            .cipher
            .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
            .map_err(|_| "Secret authentication failed")?;
        String::from_utf8(plain).map_err(|e| e.to_string())
    }
    pub fn config(&self) -> Result<Config> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        read_config(&db)
    }
    pub fn update<T>(&self, change: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
        let mut db = self.db.lock().map_err(|e| e.to_string())?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let mut cfg = read_config(&tx)?;
        let result = change(&mut cfg)?;
        write_config(&tx, &cfg)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }
    pub fn session(&self, raw: &str) -> Result<()> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        insert_session(&db, raw)
    }
    pub fn session_for_credentials(&self, raw: &str, username: &str, hash: &str) -> Result<bool> {
        let mut db = self.db.lock().map_err(|e| e.to_string())?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let cfg = read_config(&tx)?;
        if hash.is_empty() || cfg.username != username || cfg.password_hash != hash {
            return Ok(false);
        }
        insert_session(&tx, raw)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }
    pub fn change_password(&self, session: &str, old_hash: &str, new_hash: String) -> Result<bool> {
        let mut db = self.db.lock().map_err(|e| e.to_string())?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let mut cfg = read_config(&tx)?;
        let authenticated = tx
            .query_row(
                "SELECT 1 FROM sessions WHERE hash=?1 AND expires>?2",
                params![digest(session), now()],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .is_some();
        // Credential replacement and revocation share one transaction with session creation.
        if cfg.password_hash != old_hash || !authenticated {
            return Ok(false);
        }
        cfg.password_hash = new_hash;
        write_config(&tx, &cfg)?;
        tx.execute("DELETE FROM sessions", [])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }
    pub fn authenticated(&self, raw: &str) -> bool {
        self.db
            .lock()
            .ok()
            .and_then(|db| {
                db.query_row(
                    "SELECT 1 FROM sessions WHERE hash=?1 AND expires>?2",
                    params![digest(raw), now()],
                    |_| Ok(()),
                )
                .ok()
            })
            .is_some()
    }
    pub fn logout(&self, raw: &str) {
        if let Ok(db) = self.db.lock() {
            let _ = db.execute("DELETE FROM sessions WHERE hash=?1", [digest(raw)]);
        }
    }
    pub fn revoke_sessions(&self) -> Result<()> {
        self.db
            .lock()
            .map_err(|e| e.to_string())?
            .execute("DELETE FROM sessions", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn log(&self, entry: &RequestLog, retention: u32) -> Result<()> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        db.execute(
            "INSERT INTO requests VALUES(?1,?2,?3)",
            params![
                entry.id,
                entry.timestamp,
                serde_json::to_string(entry).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        db.execute(
            "DELETE FROM requests WHERE timestamp < ?1",
            [now() - retention as i64 * 86400],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn logs(&self, from: i64, to: i64) -> Result<Vec<RequestLog>> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        let mut query = db.prepare("SELECT value FROM requests WHERE timestamp>=?1 AND timestamp<=?2 ORDER BY timestamp DESC").map_err(|e| e.to_string())?;
        query
            .query_map(params![from, to], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .map(|row| {
                serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
            })
            .collect()
    }
}
pub fn usage(value: &Value) -> Usage {
    let u = value
        .get("usage")
        .or_else(|| value.pointer("/response/usage"))
        .unwrap_or(value);
    Usage {
        input_tokens: u["input_tokens"]
            .as_u64()
            .or(u["prompt_tokens"].as_u64())
            .unwrap_or(0),
        output_tokens: u["output_tokens"]
            .as_u64()
            .or(u["completion_tokens"].as_u64())
            .unwrap_or(0),
        cached_tokens: u
            .pointer("/input_tokens_details/cached_tokens")
            .or_else(|| u.pointer("/prompt_tokens_details/cached_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_storage_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let encrypted = store.seal("secret-upstream-token").unwrap();
        assert!(!encrypted.contains("secret-upstream-token"));
        assert_eq!(store.unseal(&encrypted).unwrap(), "secret-upstream-token");
        assert!(store.unseal("bad").is_err());
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.unseal(&encrypted).unwrap(), "secret-upstream-token");
    }
    #[test]
    fn sessions_are_revocable() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        s.session("abc").unwrap();
        assert!(s.authenticated("abc"));
        s.logout("abc");
        assert!(!s.authenticated("abc"));
    }
    #[test]
    fn credential_changes_atomically_revoke_and_reject_stale_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        s.update(|c| {
            c.username = "admin".into();
            c.password_hash = "first-password-hash".into();
            Ok(())
        })
        .unwrap();
        assert!(
            s.session_for_credentials("first", "admin", "first-password-hash")
                .unwrap()
        );
        assert!(
            s.change_password(
                "first",
                "first-password-hash",
                "second-password-hash".into()
            )
            .unwrap()
        );
        assert!(!s.authenticated("first"));
        assert!(
            !s.session_for_credentials("stale", "admin", "first-password-hash")
                .unwrap()
        );
        assert!(!s.authenticated("stale"));
        assert!(
            !s.change_password("first", "first-password-hash", "stale-replacement".into())
                .unwrap()
        );
        assert!(
            s.session_for_credentials("second", "admin", "second-password-hash")
                .unwrap()
        );
        s.logout("second");
        assert!(
            !s.change_password(
                "second",
                "second-password-hash",
                "revoked-replacement".into()
            )
            .unwrap()
        );
        assert_eq!(s.config().unwrap().password_hash, "second-password-hash");
    }
    #[test]
    fn usage_includes_cache() {
        let u = usage(
            &serde_json::json!({"response":{"usage":{"input_tokens":100,"output_tokens":10,"input_tokens_details":{"cached_tokens":80}}}}),
        );
        assert_eq!(u.cached_tokens, 80);
        assert_eq!(u.input_tokens, 100);
    }
}
