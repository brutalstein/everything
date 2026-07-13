use anyhow::{Context, Result};
use everything_domain::{ConnectorAuditRecord, ConnectorProvider};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConnectorConfig {
    pub provider: ConnectorProvider,
    pub client_id: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    pub configured_at_epoch_millis: u128,
    #[serde(default)]
    pub account_label: Option<String>,
    #[serde(default)]
    pub granted_scopes: Vec<String>,
    #[serde(default)]
    pub token_expires_at_epoch_millis: Option<u128>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOAuthSession {
    pub state: String,
    pub provider: ConnectorProvider,
    pub redirect_uri: String,
    pub verifier: String,
    pub created_at_epoch_millis: u128,
    pub expires_at_epoch_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredIdempotencyResult {
    pub key: String,
    pub provider: ConnectorProvider,
    pub action_id: String,
    pub created_at_epoch_millis: u128,
    pub response: Value,
}

#[derive(Debug, Clone)]
pub struct ConnectorStore {
    database_path: PathBuf,
}

impl ConnectorStore {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let store = Self {
            database_path: path.into(),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn upsert_config(&self, config: &StoredConnectorConfig) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO connector_configs (
                provider, configured_at, token_expires_at, config_json
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider) DO UPDATE SET
                configured_at = excluded.configured_at,
                token_expires_at = excluded.token_expires_at,
                config_json = excluded.config_json",
            params![
                config.provider.as_str(),
                millis_as_i64(config.configured_at_epoch_millis),
                config.token_expires_at_epoch_millis.map(millis_as_i64),
                serde_json::to_string(config)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_config(&self, provider: ConnectorProvider) -> Result<Option<StoredConnectorConfig>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT config_json FROM connector_configs WHERE provider = ?1",
                params![provider.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|json| serde_json::from_str(&json).context("connector config is corrupt"))
            .transpose()
    }

    pub fn list_configs(&self) -> Result<Vec<StoredConnectorConfig>> {
        let connection = self.open()?;
        let mut statement =
            connection.prepare("SELECT config_json FROM connector_configs ORDER BY provider")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut configs = Vec::new();
        for row in rows {
            if let Ok(config) = serde_json::from_str::<StoredConnectorConfig>(&row?) {
                configs.push(config);
            }
        }
        Ok(configs)
    }

    pub fn delete_config(&self, provider: ConnectorProvider) -> Result<bool> {
        let connection = self.open()?;
        Ok(connection.execute(
            "DELETE FROM connector_configs WHERE provider = ?1",
            params![provider.as_str()],
        )? > 0)
    }

    pub fn save_oauth_session(&self, session: &StoredOAuthSession) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO oauth_sessions (state, provider, expires_at, session_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(state) DO UPDATE SET
                provider = excluded.provider,
                expires_at = excluded.expires_at,
                session_json = excluded.session_json",
            params![
                session.state,
                session.provider.as_str(),
                millis_as_i64(session.expires_at_epoch_millis),
                serde_json::to_string(session)?,
            ],
        )?;
        Ok(())
    }

    pub fn consume_oauth_session(
        &self,
        provider: ConnectorProvider,
        state: &str,
        now: u128,
    ) -> Result<Option<StoredOAuthSession>> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        let payload = transaction
            .query_row(
                "SELECT session_json FROM oauth_sessions
                 WHERE state = ?1 AND provider = ?2 AND expires_at >= ?3",
                params![state, provider.as_str(), millis_as_i64(now)],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if payload.is_some() {
            transaction.execute(
                "DELETE FROM oauth_sessions WHERE state = ?1",
                params![state],
            )?;
        }
        transaction.execute(
            "DELETE FROM oauth_sessions WHERE expires_at < ?1",
            params![millis_as_i64(now)],
        )?;
        transaction.commit()?;
        payload
            .map(|json| serde_json::from_str(&json).context("OAuth session is corrupt"))
            .transpose()
    }

    pub fn take_expired_oauth_sessions(&self, now: u128) -> Result<Vec<StoredOAuthSession>> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        let payloads = {
            let mut statement = transaction
                .prepare("SELECT session_json FROM oauth_sessions WHERE expires_at < ?1")?;
            statement
                .query_map(params![millis_as_i64(now)], |row| row.get::<_, String>(0))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>()
        };
        transaction.execute(
            "DELETE FROM oauth_sessions WHERE expires_at < ?1",
            params![millis_as_i64(now)],
        )?;
        transaction.commit()?;
        Ok(payloads
            .into_iter()
            .filter_map(|payload| serde_json::from_str::<StoredOAuthSession>(&payload).ok())
            .collect())
    }

    pub fn save_audit(&self, audit: &ConnectorAuditRecord) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO connector_audit (
                audit_id, provider, action_id, started_at, finished_at, status,
                idempotency_key, audit_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                audit.audit_id,
                audit.provider.as_str(),
                audit.action_id,
                millis_as_i64(audit.started_at_epoch_millis),
                millis_as_i64(audit.finished_at_epoch_millis),
                audit.status,
                audit.idempotency_key,
                serde_json::to_string(audit)?,
            ],
        )?;
        Ok(())
    }

    pub fn list_audits(&self, limit: usize) -> Result<Vec<ConnectorAuditRecord>> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare("SELECT audit_json FROM connector_audit ORDER BY started_at DESC LIMIT ?1")?;
        let rows = statement.query_map(
            params![i64::try_from(limit.clamp(1, 1000)).unwrap_or(1000)],
            |row| row.get::<_, String>(0),
        )?;
        let mut audits = Vec::new();
        for row in rows {
            if let Ok(audit) = serde_json::from_str::<ConnectorAuditRecord>(&row?) {
                audits.push(audit);
            }
        }
        Ok(audits)
    }

    pub fn save_idempotency(&self, record: &StoredIdempotencyResult) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO connector_idempotency (
                idempotency_key, provider, action_id, created_at, response_json
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(idempotency_key) DO NOTHING",
            params![
                record.key,
                record.provider.as_str(),
                record.action_id,
                millis_as_i64(record.created_at_epoch_millis),
                serde_json::to_string(record)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_idempotency(&self, key: &str) -> Result<Option<StoredIdempotencyResult>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT response_json FROM connector_idempotency WHERE idempotency_key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|json| serde_json::from_str(&json).context("idempotency record is corrupt"))
            .transpose()
    }

    fn initialize(&self) -> Result<()> {
        if let Some(parent) = self.database_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = self.open()?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS connector_metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS connector_configs (
                provider TEXT PRIMARY KEY,
                configured_at INTEGER NOT NULL,
                token_expires_at INTEGER,
                config_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS oauth_sessions (
                state TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                session_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS connector_audit (
                audit_id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                action_id TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER NOT NULL,
                status TEXT NOT NULL,
                idempotency_key TEXT,
                audit_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS connector_idempotency (
                idempotency_key TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                action_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                response_json TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_connector_audit_started
                ON connector_audit(started_at DESC);
             CREATE INDEX IF NOT EXISTS idx_oauth_sessions_expiry
                ON oauth_sessions(expires_at);",
        )?;
        let existing = connection
            .query_row(
                "SELECT value FROM connector_metadata WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        anyhow::ensure!(
            existing <= SCHEMA_VERSION,
            "connector database schema {existing} is newer than supported schema {SCHEMA_VERSION}"
        );
        connection.execute(
            "INSERT INTO connector_metadata (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        let connection = Connection::open(&self.database_path)
            .with_context(|| format!("open connector database {}", self.database_path.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }
}

fn millis_as_i64(value: u128) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
#[cfg(test)]
mod tests {
    use super::{ConnectorStore, StoredOAuthSession};
    use everything_domain::ConnectorProvider;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn store(label: &str) -> ConnectorStore {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-connectors-{label}-{stamp}"));
        fs::create_dir_all(&root).expect("create temp root");
        ConnectorStore::new(root.join("connectors.sqlite3")).expect("store")
    }

    fn session(state: &str, provider: ConnectorProvider, expires_at: u128) -> StoredOAuthSession {
        StoredOAuthSession {
            state: state.to_owned(),
            provider,
            redirect_uri: "http://127.0.0.1:43821/callback".to_owned(),
            verifier: String::new(),
            created_at_epoch_millis: 1,
            expires_at_epoch_millis: expires_at,
        }
    }

    #[test]
    fn wrong_provider_does_not_consume_oauth_state() {
        let store = store("provider");
        store
            .save_oauth_session(&session("state", ConnectorProvider::Gmail, 100))
            .expect("save");
        assert!(
            store
                .consume_oauth_session(ConnectorProvider::Spotify, "state", 50)
                .expect("wrong provider")
                .is_none()
        );
        let consumed = store
            .consume_oauth_session(ConnectorProvider::Gmail, "state", 50)
            .expect("right provider")
            .expect("session remains");
        assert_eq!(consumed.provider, ConnectorProvider::Gmail);
    }

    #[test]
    fn expired_oauth_sessions_are_returned_once() {
        let store = store("expired");
        store
            .save_oauth_session(&session("old", ConnectorProvider::TikTok, 10))
            .expect("save old");
        store
            .save_oauth_session(&session("fresh", ConnectorProvider::Spotify, 100))
            .expect("save fresh");
        let expired = store.take_expired_oauth_sessions(50).expect("expired");
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].state, "old");
        assert!(
            store
                .take_expired_oauth_sessions(50)
                .expect("second")
                .is_empty()
        );
        assert!(
            store
                .consume_oauth_session(ConnectorProvider::Spotify, "fresh", 50)
                .expect("consume fresh")
                .is_some()
        );
    }
}
