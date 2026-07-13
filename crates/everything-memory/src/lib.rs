use anyhow::{Context, Result, anyhow};
use everything_domain::{
    MemoryEntry, MemoryId, MemoryQuery, MemoryScope, MemorySearchResult, MemoryUpsertRequest,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MEMORY_SCHEMA_VERSION: i64 = 3;
static MEMORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct MemoryStore {
    database_path: PathBuf,
}

impl MemoryStore {
    pub fn new(database_path: impl Into<PathBuf>) -> Result<Self> {
        let store = Self {
            database_path: database_path.into(),
        };
        store.initialize()?;
        store.compact_expired()?;
        Ok(store)
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn upsert(&self, mut request: MemoryUpsertRequest) -> Result<MemoryEntry> {
        validate_request(&request)?;
        let duplicate_id = if request.memory_id.is_none() {
            self.find_duplicate(&request)?
        } else {
            None
        };
        if let Some(memory_id) = duplicate_id.as_ref() {
            if let Some(existing) = self.get(memory_id.as_str())? {
                // Exact repeats from deterministic automation outcomes should be idempotent,
                // including immutable audit-style memories.
                if !existing.editable {
                    return Ok(existing);
                }
            }
            request.memory_id = duplicate_id;
        }
        let now = now_millis();
        let existing = request
            .memory_id
            .as_ref()
            .map(|id| self.get(id.as_str()))
            .transpose()?
            .flatten();
        if let Some(entry) = existing.as_ref() {
            anyhow::ensure!(
                entry.editable,
                "memory '{}' is not editable",
                entry.memory_id
            );
        }
        let existing_created_at = existing
            .as_ref()
            .map_or(now, |entry| entry.created_at_epoch_millis);
        let existing_superseded_by = existing
            .as_ref()
            .and_then(|entry| entry.superseded_by.clone());
        let memory_id = request.memory_id.unwrap_or_else(|| {
            MemoryId::new(format!(
                "memory-{now}-{}",
                MEMORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ))
        });
        let entry = MemoryEntry {
            memory_id,
            scope: request.scope,
            title: request.title.trim().to_owned(),
            content: request.content.trim().to_owned(),
            source: request.source.trim().to_owned(),
            workspace_key: request.workspace_key,
            run_id: request.run_id,
            artifact_id: request.artifact_id,
            valid_from_epoch_millis: request.valid_from_epoch_millis,
            valid_until_epoch_millis: request.valid_until_epoch_millis,
            version: request.version,
            confidence: request.confidence,
            evidence_ids: request.evidence_ids,
            tags: request
                .tags
                .into_iter()
                .map(|tag| tag.trim().to_owned())
                .filter(|tag| !tag.is_empty())
                .collect(),
            superseded_by: existing_superseded_by,
            editable: request.editable,
            forgettable: request.forgettable,
            created_at_epoch_millis: existing_created_at,
            updated_at_epoch_millis: now,
        };
        self.save_entry(&entry)?;
        Ok(entry)
    }

    fn find_duplicate(&self, request: &MemoryUpsertRequest) -> Result<Option<MemoryId>> {
        let connection = self.open()?;
        let fingerprint = request_fingerprint(request);
        let memory_id = connection
            .query_row(
                "SELECT memory_id FROM memory_entries \
                 WHERE scope = ?1 \
                   AND (?2 IS NULL AND workspace_key IS NULL OR workspace_key = ?2) \
                   AND content_hash = ?3 \
                   AND superseded_by IS NULL \
                 ORDER BY updated_at DESC LIMIT 1",
                params![
                    scope_name(request.scope),
                    request.workspace_key.as_deref(),
                    fingerprint
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(memory_id.map(MemoryId::new))
    }

    pub fn compact_expired(&self) -> Result<usize> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        let now = millis_as_i64(now_millis());
        let ids = {
            let mut statement = transaction.prepare(
                "SELECT memory_id FROM memory_entries \
                 WHERE valid_until IS NOT NULL AND valid_until < ?1 AND forgettable = 1",
            )?;
            statement
                .query_map(params![now], |row| row.get::<_, String>(0))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>()
        };
        for memory_id in &ids {
            transaction.execute(
                "DELETE FROM memory_fts WHERE memory_id = ?1",
                params![memory_id],
            )?;
            transaction.execute(
                "DELETE FROM memory_entries WHERE memory_id = ?1",
                params![memory_id],
            )?;
        }
        transaction.commit()?;
        if !ids.is_empty() {
            connection.execute_batch("INSERT INTO memory_fts(memory_fts) VALUES('optimize');")?;
        }
        Ok(ids.len())
    }

    pub fn get(&self, memory_id: &str) -> Result<Option<MemoryEntry>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT entry_json FROM memory_entries WHERE memory_id = ?1",
                params![memory_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| {
                serde_json::from_str(&payload)
                    .with_context(|| format!("memory record '{memory_id}' is corrupt"))
            })
            .transpose()
    }

    pub fn search(&self, query: &MemoryQuery) -> Result<Vec<MemorySearchResult>> {
        anyhow::ensure!(
            query.query.chars().count() <= 4096,
            "memory search query exceeds 4096 characters"
        );
        anyhow::ensure!(
            (1..=200).contains(&query.limit),
            "memory search limit must be between 1 and 200"
        );
        if let Some(workspace_key) = query.workspace_key.as_deref() {
            anyhow::ensure!(
                workspace_key.chars().count() <= 4096,
                "memory workspace key exceeds 4096 characters"
            );
        }
        let limit = query.limit;
        if query.query.trim().is_empty() {
            return self.list_without_fts(query, limit);
        }
        let connection = self.open()?;
        let scope = query.scope.map(scope_name);
        let workspace = query.workspace_key.as_deref();
        let include_superseded = query.include_superseded;
        let fts_query = build_fts_query(&query.query);
        if fts_query.is_empty() {
            return self.list_without_fts(query, limit);
        }
        let mut statement = connection.prepare(
            "SELECT e.entry_json, bm25(memory_fts) AS rank \
             FROM memory_fts \
             JOIN memory_entries e ON e.memory_id = memory_fts.memory_id \
             WHERE memory_fts MATCH ?1 \
               AND (?2 IS NULL OR e.scope = ?2) \
               AND (?3 IS NULL OR e.workspace_key = ?3) \
               AND (?4 = 1 OR e.superseded_by IS NULL) \
               AND (e.valid_from IS NULL OR e.valid_from <= ?5) \
               AND (e.valid_until IS NULL OR e.valid_until >= ?5) \
             ORDER BY rank, e.updated_at DESC LIMIT ?6",
        )?;
        let rows = statement.query_map(
            params![
                fts_query,
                scope,
                workspace,
                if include_superseded { 1_i64 } else { 0_i64 },
                millis_as_i64(now_millis()),
                i64::try_from(limit).unwrap_or(200),
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
        )?;
        let mut results = Vec::new();
        for row in rows {
            let (payload, rank) = row?;
            if let Ok(entry) = serde_json::from_str::<MemoryEntry>(&payload) {
                let score = memory_score(&entry, rank);
                results.push(MemorySearchResult { entry, score });
            }
        }
        Ok(results)
    }

    pub fn forget(&self, memory_id: &str) -> Result<bool> {
        let Some(entry) = self.get(memory_id)? else {
            return Ok(false);
        };
        anyhow::ensure!(entry.forgettable, "memory '{memory_id}' is not forgettable");
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM memory_fts WHERE memory_id = ?1",
            params![memory_id],
        )?;
        let deleted = transaction.execute(
            "DELETE FROM memory_entries WHERE memory_id = ?1",
            params![memory_id],
        )?;
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn supersede(&self, old_memory_id: &str, new_memory_id: &str) -> Result<()> {
        anyhow::ensure!(
            old_memory_id != new_memory_id,
            "memory cannot supersede itself"
        );
        let Some(mut old) = self.get(old_memory_id)? else {
            return Err(anyhow!("memory '{old_memory_id}' not found"));
        };
        anyhow::ensure!(
            self.get(new_memory_id)?.is_some(),
            "replacement memory not found"
        );
        anyhow::ensure!(old.editable, "memory '{old_memory_id}' is not editable");
        old.superseded_by = Some(MemoryId::new(new_memory_id));
        old.updated_at_epoch_millis = now_millis();
        self.save_entry(&old)
    }

    fn save_entry(&self, entry: &MemoryEntry) -> Result<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO memory_entries ( \
                memory_id, scope, workspace_key, superseded_by, updated_at, content_hash, \
                confidence, valid_from, valid_until, forgettable, entry_json \
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
             ON CONFLICT(memory_id) DO UPDATE SET \
                scope = excluded.scope, \
                workspace_key = excluded.workspace_key, \
                superseded_by = excluded.superseded_by, \
                updated_at = excluded.updated_at, \
                content_hash = excluded.content_hash, \
                confidence = excluded.confidence, \
                valid_from = excluded.valid_from, \
                valid_until = excluded.valid_until, \
                forgettable = excluded.forgettable, \
                entry_json = excluded.entry_json",
            params![
                entry.memory_id.as_str(),
                scope_name(entry.scope),
                entry.workspace_key.as_deref(),
                entry.superseded_by.as_ref().map(MemoryId::as_str),
                millis_as_i64(entry.updated_at_epoch_millis),
                entry_fingerprint(entry),
                entry.confidence,
                entry.valid_from_epoch_millis.map(millis_as_i64),
                entry.valid_until_epoch_millis.map(millis_as_i64),
                if entry.forgettable { 1_i64 } else { 0_i64 },
                serde_json::to_string(entry)?,
            ],
        )?;
        transaction.execute(
            "DELETE FROM memory_fts WHERE memory_id = ?1",
            params![entry.memory_id.as_str()],
        )?;
        transaction.execute(
            "INSERT INTO memory_fts (memory_id, title, content, tags) VALUES (?1, ?2, ?3, ?4)",
            params![
                entry.memory_id.as_str(),
                &entry.title,
                &entry.content,
                entry.tags.join(" "),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn list_without_fts(
        &self,
        query: &MemoryQuery,
        limit: usize,
    ) -> Result<Vec<MemorySearchResult>> {
        let connection = self.open()?;
        let scope = query.scope.map(scope_name);
        let workspace = query.workspace_key.as_deref();
        let mut statement = connection.prepare(
            "SELECT entry_json FROM memory_entries \
             WHERE (?1 IS NULL OR scope = ?1) \
               AND (?2 IS NULL OR workspace_key = ?2) \
               AND (?3 = 1 OR superseded_by IS NULL) \
               AND (valid_from IS NULL OR valid_from <= ?4) \
               AND (valid_until IS NULL OR valid_until >= ?4) \
             ORDER BY updated_at DESC LIMIT ?5",
        )?;
        let rows = statement.query_map(
            params![
                scope,
                workspace,
                if query.include_superseded {
                    1_i64
                } else {
                    0_i64
                },
                millis_as_i64(now_millis()),
                i64::try_from(limit).unwrap_or(200),
            ],
            |row| row.get::<_, String>(0),
        )?;
        let mut results = Vec::new();
        for row in rows {
            if let Ok(entry) = serde_json::from_str::<MemoryEntry>(&row?) {
                let score = memory_score(&entry, 0.0);
                results.push(MemorySearchResult { entry, score });
            }
        }
        Ok(results)
    }

    fn initialize(&self) -> Result<()> {
        if let Some(parent) = self.database_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = self.open()?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_metadata ( \
                key TEXT PRIMARY KEY, value TEXT NOT NULL \
             ); \
             CREATE TABLE IF NOT EXISTS memory_entries ( \
                memory_id TEXT PRIMARY KEY, \
                scope TEXT NOT NULL, \
                workspace_key TEXT, \
                superseded_by TEXT, \
                updated_at INTEGER NOT NULL, \
                content_hash TEXT NOT NULL DEFAULT '', \
                confidence REAL NOT NULL DEFAULT 1.0, \
                valid_from INTEGER, \
                valid_until INTEGER, \
                forgettable INTEGER NOT NULL DEFAULT 1, \
                entry_json TEXT NOT NULL \
             ); \
             CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5( \
                memory_id UNINDEXED, title, content, tags \
             ); \
             CREATE INDEX IF NOT EXISTS idx_memory_scope_updated \
                ON memory_entries(scope, updated_at DESC); \
             CREATE INDEX IF NOT EXISTS idx_memory_workspace_updated \
                ON memory_entries(workspace_key, updated_at DESC);",
        )?;
        ensure_column(
            &connection,
            "memory_entries",
            "content_hash",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &connection,
            "memory_entries",
            "confidence",
            "REAL NOT NULL DEFAULT 1.0",
        )?;
        ensure_column(&connection, "memory_entries", "valid_from", "INTEGER")?;
        ensure_column(&connection, "memory_entries", "valid_until", "INTEGER")?;
        ensure_column(
            &connection,
            "memory_entries",
            "forgettable",
            "INTEGER NOT NULL DEFAULT 1",
        )?;
        backfill_memory_metadata(&connection)?;
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_memory_fingerprint \
                ON memory_entries(scope, workspace_key, content_hash); \
             CREATE INDEX IF NOT EXISTS idx_memory_validity \
                ON memory_entries(valid_until, valid_from);",
        )?;
        let existing_version = connection
            .query_row(
                "SELECT value FROM memory_metadata WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        anyhow::ensure!(
            existing_version <= MEMORY_SCHEMA_VERSION,
            "memory database schema {existing_version} is newer than supported schema {MEMORY_SCHEMA_VERSION}"
        );
        connection.execute(
            "INSERT INTO memory_metadata (key, value) VALUES ('schema_version', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![MEMORY_SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        let connection = Connection::open(&self.database_path)
            .with_context(|| format!("open memory database {}", self.database_path.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }
}

fn validate_request(request: &MemoryUpsertRequest) -> Result<()> {
    anyhow::ensure!(
        !request.title.trim().is_empty(),
        "memory title must not be empty"
    );
    anyhow::ensure!(
        !request.content.trim().is_empty(),
        "memory content must not be empty"
    );
    anyhow::ensure!(
        !request.source.trim().is_empty(),
        "memory source must not be empty"
    );
    anyhow::ensure!(
        request.title.chars().count() <= 512,
        "memory title exceeds 512 characters"
    );
    anyhow::ensure!(
        request.content.len() <= 256 * 1024,
        "memory content exceeds 256 KiB"
    );
    anyhow::ensure!(
        request.source.chars().count() <= 256,
        "memory source exceeds 256 characters"
    );
    anyhow::ensure!(request.tags.len() <= 64, "memory has too many tags");
    anyhow::ensure!(
        request.evidence_ids.len() <= 64,
        "memory has too many evidence references"
    );
    anyhow::ensure!(
        request
            .tags
            .iter()
            .all(|tag| !tag.trim().is_empty() && tag.chars().count() <= 64),
        "memory tags must be non-empty and at most 64 characters"
    );
    anyhow::ensure!(
        request
            .evidence_ids
            .iter()
            .all(|evidence| !evidence.as_str().trim().is_empty()
                && evidence.as_str().chars().count() <= 128),
        "memory evidence identifiers must be non-empty and at most 128 characters"
    );
    if let Some(workspace_key) = request.workspace_key.as_deref() {
        anyhow::ensure!(
            !workspace_key.trim().is_empty() && workspace_key.chars().count() <= 4096,
            "memory workspace key must be non-empty and at most 4096 characters"
        );
    }
    if let Some(memory_id) = request.memory_id.as_ref() {
        anyhow::ensure!(
            !memory_id.as_str().trim().is_empty() && memory_id.as_str().chars().count() <= 128,
            "memory identifier must be non-empty and at most 128 characters"
        );
    }
    anyhow::ensure!(
        request.version > 0,
        "memory version must be greater than zero"
    );
    anyhow::ensure!(
        request.confidence.is_finite(),
        "memory confidence must be finite"
    );
    anyhow::ensure!(
        (0.0..=1.0).contains(&request.confidence),
        "memory confidence must be between 0 and 1"
    );
    if let (Some(from), Some(until)) = (
        request.valid_from_epoch_millis,
        request.valid_until_epoch_millis,
    ) {
        anyhow::ensure!(from <= until, "memory validity range is inverted");
    }
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    if !columns.iter().any(|name| name == column) {
        connection.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))?;
    }
    Ok(())
}

fn backfill_memory_metadata(connection: &Connection) -> Result<()> {
    let mut statement = connection
        .prepare("SELECT memory_id, entry_json FROM memory_entries WHERE content_hash = ''")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    for (memory_id, payload) in rows {
        let Ok(entry) = serde_json::from_str::<MemoryEntry>(&payload) else {
            continue;
        };
        connection.execute(
            "UPDATE memory_entries SET content_hash = ?1, confidence = ?2, valid_from = ?3, valid_until = ?4, forgettable = ?5 WHERE memory_id = ?6",
            params![
                entry_fingerprint(&entry),
                entry.confidence,
                entry.valid_from_epoch_millis.map(millis_as_i64),
                entry.valid_until_epoch_millis.map(millis_as_i64),
                if entry.forgettable { 1_i64 } else { 0_i64 },
                memory_id,
            ],
        )?;
    }
    Ok(())
}

fn request_fingerprint(request: &MemoryUpsertRequest) -> String {
    fingerprint_parts([
        scope_name(request.scope),
        request.workspace_key.as_deref().unwrap_or_default(),
        request.source.trim(),
        request.title.trim(),
        request.content.trim(),
    ])
}

fn entry_fingerprint(entry: &MemoryEntry) -> String {
    fingerprint_parts([
        scope_name(entry.scope),
        entry.workspace_key.as_deref().unwrap_or_default(),
        entry.source.trim(),
        entry.title.trim(),
        entry.content.trim(),
    ])
}

fn fingerprint_parts<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in parts {
        for byte in part.as_bytes().iter().copied().chain(std::iter::once(0xff)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

fn memory_score(entry: &MemoryEntry, rank: f64) -> f64 {
    let lexical = 1.0 / (1.0 + rank.abs());
    let age_days = now_millis().saturating_sub(entry.updated_at_epoch_millis) as f64 / 86_400_000.0;
    let recency = 1.0 / (1.0 + age_days / 30.0);
    let scope_weight = match entry.scope {
        MemoryScope::Preference => 1.0,
        MemoryScope::Workspace => 0.95,
        MemoryScope::Graph => 0.90,
        MemoryScope::Task => 0.85,
        MemoryScope::Artifact => 0.80,
        MemoryScope::Session => 0.75,
    };
    (lexical * 0.50 + f64::from(entry.confidence) * 0.30 + recency * 0.20) * scope_weight
}

fn scope_name(scope: MemoryScope) -> &'static str {
    match scope {
        MemoryScope::Session => "Session",
        MemoryScope::Workspace => "Workspace",
        MemoryScope::Task => "Task",
        MemoryScope::Artifact => "Artifact",
        MemoryScope::Graph => "Graph",
        MemoryScope::Preference => "Preference",
    }
}

fn build_fts_query(value: &str) -> String {
    let mut terms = value
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .map(str::trim)
        .filter(|term| term.chars().count() >= 2)
        .map(|term| term.to_lowercase())
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
        .into_iter()
        .take(16)
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn millis_as_i64(value: u128) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::MemoryStore;
    use everything_domain::{MemoryQuery, MemoryScope, MemoryUpsertRequest};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("everything-memory-{label}-{stamp}"));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn request(title: &str, content: &str) -> MemoryUpsertRequest {
        MemoryUpsertRequest {
            memory_id: None,
            scope: MemoryScope::Workspace,
            title: title.to_owned(),
            content: content.to_owned(),
            source: "test".to_owned(),
            workspace_key: Some("workspace".to_owned()),
            run_id: None,
            artifact_id: None,
            valid_from_epoch_millis: None,
            valid_until_epoch_millis: None,
            version: 1,
            confidence: 0.9,
            evidence_ids: Vec::new(),
            tags: vec!["rust".to_owned()],
            editable: true,
            forgettable: true,
        }
    }

    #[test]
    fn memory_round_trips_and_is_searchable() {
        let store = MemoryStore::new(root("search").join("memory.sqlite3")).expect("store");
        let entry = store
            .upsert(request(
                "Graph source",
                "Persistent SQLite graph is canonical",
            ))
            .expect("upsert");
        let loaded = store
            .get(entry.memory_id.as_str())
            .expect("get")
            .expect("entry");
        assert_eq!(loaded.title, "Graph source");
        let results = store
            .search(&MemoryQuery {
                query: "SQLite graph".to_owned(),
                scope: Some(MemoryScope::Workspace),
                workspace_key: Some("workspace".to_owned()),
                limit: 10,
                include_superseded: false,
            })
            .expect("search");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn memory_search_matches_non_contiguous_terms() {
        let store = MemoryStore::new(root("search-terms").join("memory.sqlite3")).expect("store");
        store
            .upsert(request(
                "Runtime policy",
                "Persistent graph retrieval keeps canonical source evidence bounded",
            ))
            .expect("upsert");
        let results = store
            .search(&MemoryQuery {
                query: "canonical bounded".to_owned(),
                scope: Some(MemoryScope::Workspace),
                workspace_key: Some("workspace".to_owned()),
                limit: 10,
                include_superseded: false,
            })
            .expect("search");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn punctuation_only_query_falls_back_without_fts_error() {
        let store =
            MemoryStore::new(root("search-punctuation").join("memory.sqlite3")).expect("store");
        store.upsert(request("Entry", "Content")).expect("upsert");
        let results = store
            .search(&MemoryQuery {
                query: "---".to_owned(),
                scope: Some(MemoryScope::Workspace),
                workspace_key: Some("workspace".to_owned()),
                limit: 10,
                include_superseded: false,
            })
            .expect("search");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn oversized_metadata_and_invalid_search_limits_are_rejected() {
        let store =
            MemoryStore::new(root("metadata-validation").join("memory.sqlite3")).expect("store");

        let mut long_tag = request("Tags", "Must be bounded");
        long_tag.tags = vec!["x".repeat(65)];
        assert!(store.upsert(long_tag).is_err());

        let mut long_workspace = request("Workspace", "Must be bounded");
        long_workspace.workspace_key = Some("w".repeat(4097));
        assert!(store.upsert(long_workspace).is_err());

        assert!(
            store
                .search(&MemoryQuery {
                    query: "x".to_owned(),
                    scope: None,
                    workspace_key: None,
                    limit: 0,
                    include_superseded: false,
                })
                .is_err()
        );
        assert!(
            store
                .search(&MemoryQuery {
                    query: "x".repeat(4097),
                    scope: None,
                    workspace_key: None,
                    limit: 20,
                    include_superseded: false,
                })
                .is_err()
        );
    }

    #[test]
    fn invalid_version_confidence_and_validity_are_rejected() {
        let store = MemoryStore::new(root("validation").join("memory.sqlite3")).expect("store");

        let mut invalid_version = request("Version", "Must be positive");
        invalid_version.version = 0;
        assert!(store.upsert(invalid_version).is_err());

        let mut invalid_confidence = request("Confidence", "Must be bounded");
        invalid_confidence.confidence = 1.5;
        assert!(store.upsert(invalid_confidence).is_err());

        let mut invalid_validity = request("Validity", "Must be ordered");
        invalid_validity.valid_from_epoch_millis = Some(20);
        invalid_validity.valid_until_epoch_millis = Some(10);
        assert!(store.upsert(invalid_validity).is_err());
    }

    #[test]
    fn forget_respects_policy() {
        let store = MemoryStore::new(root("forget").join("memory.sqlite3")).expect("store");
        let mut immutable = request("Policy", "Do not remove");
        immutable.forgettable = false;
        let entry = store.upsert(immutable).expect("upsert");
        assert!(store.forget(entry.memory_id.as_str()).is_err());
    }

    #[test]
    fn expired_non_forgettable_memory_is_retained() {
        let path = root("retention").join("memory.sqlite3");
        let store = MemoryStore::new(&path).expect("store");
        let mut retained = request("Audit", "Retain after validity window");
        retained.valid_until_epoch_millis = Some(1);
        retained.forgettable = false;
        let entry = store.upsert(retained).expect("upsert");
        assert_eq!(store.compact_expired().expect("compact"), 0);
        assert!(store.get(entry.memory_id.as_str()).expect("get").is_some());
    }

    #[test]
    fn immutable_exact_duplicate_is_idempotent() {
        let store = MemoryStore::new(root("dedupe").join("memory.sqlite3")).expect("store");
        let mut immutable = request("Outcome", "Same deterministic result");
        immutable.editable = false;
        let first = store.upsert(immutable.clone()).expect("first");
        let second = store.upsert(immutable).expect("duplicate");
        assert_eq!(first.memory_id, second.memory_id);
    }
}
