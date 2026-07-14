use anyhow::{Context, Result};
use everything_domain::{
    ArtifactDescriptor, AutomationDefinition, AutomationExecution, Checkpoint, ErrorCode,
    FailureClass, InvocationStatus, ModelInvocationRecord, RunEvent, RunJournal,
    ToolInvocationRecord,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const STATE_SCHEMA_VERSION: i64 = 8;
static AUTOMATION_LEASE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct StateStore {
    database_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AutomationLease {
    pub automation: AutomationDefinition,
    pub lease_owner: String,
    pub lease_until_epoch_millis: u128,
}

#[derive(Debug)]
pub struct JournalLoadReport {
    pub entries: Vec<(RunJournal, PathBuf)>,
    pub corrupt_run_ids: Vec<String>,
}

#[derive(Debug)]
pub struct EventLoadReport {
    pub entries: Vec<RunEvent>,
    pub corrupt_sequences: Vec<u64>,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct LegacyImportReport {
    pub imported: usize,
    pub skipped_existing: usize,
    pub corrupt_files: usize,
}

impl StateStore {
    pub fn new(database_path: impl Into<PathBuf>) -> Result<Self> {
        let store = Self {
            database_path: database_path.into(),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn save_journal(&self, journal: &RunJournal) -> Result<PathBuf> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        let journal_json = serde_json::to_string(journal)?;

        transaction.execute(
            "INSERT INTO runs ( \
                run_id, objective, status, generated_by, created_at, updated_at, journal_json \
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(run_id) DO UPDATE SET \
                objective = excluded.objective, \
                status = excluded.status, \
                generated_by = excluded.generated_by, \
                created_at = excluded.created_at, \
                updated_at = excluded.updated_at, \
                journal_json = excluded.journal_json",
            params![
                journal.run_id,
                journal.objective,
                format!("{:?}", journal.status),
                journal.generated_by,
                millis_as_i64(journal.created_at_epoch_millis),
                millis_as_i64(journal.updated_at_epoch_millis),
                journal_json,
            ],
        )?;

        for event in &journal.events {
            transaction.execute(
                "INSERT OR IGNORE INTO run_events ( \
                    run_id, sequence, event_id, timestamp, event_kind, stage, event_json \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    journal.run_id,
                    i64::try_from(event.sequence).unwrap_or(i64::MAX),
                    event.event_id,
                    millis_as_i64(event.timestamp_epoch_millis),
                    event.event_kind,
                    event.stage,
                    serde_json::to_string(event)?,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(self.database_path.clone())
    }

    pub fn get_journal(&self, run_id: &str) -> Result<Option<RunJournal>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT journal_json FROM runs WHERE run_id = ?1",
                params![run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        let mut journal: Option<RunJournal> = payload
            .map(|payload| {
                serde_json::from_str::<RunJournal>(&payload)
                    .with_context(|| format!("state record for run '{run_id}' is corrupt"))
            })
            .transpose()?;

        if let Some(journal) = journal.as_mut() {
            let events = self.load_events(run_id)?;
            if !events.entries.is_empty() {
                journal.events = events.entries;
            }
        }
        Ok(journal)
    }

    pub fn load_events(&self, run_id: &str) -> Result<EventLoadReport> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT sequence, event_json FROM run_events WHERE run_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map(params![run_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut entries = Vec::new();
        let mut corrupt_sequences = Vec::new();
        for row in rows {
            let (sequence, payload) = row?;
            match serde_json::from_str::<RunEvent>(&payload) {
                Ok(event) => entries.push(event),
                Err(_) => corrupt_sequences.push(u64::try_from(sequence).unwrap_or_default()),
            }
        }
        Ok(EventLoadReport {
            entries,
            corrupt_sequences,
        })
    }

    pub fn load_journals(&self) -> Result<JournalLoadReport> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT run_id, journal_json FROM runs ORDER BY updated_at DESC, run_id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut entries = Vec::new();
        let mut corrupt_run_ids = Vec::new();
        for row in rows {
            let (run_id, payload) = row?;
            match serde_json::from_str::<RunJournal>(&payload) {
                Ok(journal) => entries.push((journal, self.database_path.clone())),
                Err(_) => corrupt_run_ids.push(run_id),
            }
        }

        Ok(JournalLoadReport {
            entries,
            corrupt_run_ids,
        })
    }

    pub fn import_legacy_json_dir(&self, runs_dir: &Path) -> Result<LegacyImportReport> {
        if !runs_dir.exists() {
            return Ok(LegacyImportReport::default());
        }

        let mut report = LegacyImportReport::default();
        for entry in fs::read_dir(runs_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }

            let payload = match fs::read_to_string(&path) {
                Ok(payload) => payload,
                Err(_) => {
                    report.corrupt_files += 1;
                    continue;
                }
            };
            let journal = match serde_json::from_str::<RunJournal>(&payload) {
                Ok(journal) => journal,
                Err(_) => {
                    report.corrupt_files += 1;
                    continue;
                }
            };

            if self.get_journal(&journal.run_id)?.is_some() {
                report.skipped_existing += 1;
                continue;
            }

            self.save_journal(&journal)?;
            report.imported += 1;
        }

        Ok(report)
    }

    pub fn save_model_invocation(&self, invocation: &ModelInvocationRecord) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO model_invocations (
                invocation_id, run_id, provider, model_name, status, started_at, finished_at, record_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(invocation_id) DO UPDATE SET
                provider = excluded.provider,
                model_name = excluded.model_name,
                status = excluded.status,
                finished_at = excluded.finished_at,
                record_json = excluded.record_json",
            params![
                invocation.invocation_id.as_str(),
                invocation.run_id.as_str(),
                invocation.provider,
                invocation.model,
                format!("{:?}", invocation.status),
                millis_as_i64(invocation.started_at_epoch_millis),
                invocation.finished_at_epoch_millis.map(millis_as_i64),
                serde_json::to_string(invocation)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_model_invocation(
        &self,
        invocation_id: &str,
    ) -> Result<Option<ModelInvocationRecord>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT record_json FROM model_invocations WHERE invocation_id = ?1",
                params![invocation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| {
                serde_json::from_str(&payload)
                    .with_context(|| format!("model invocation '{invocation_id}' is corrupt"))
            })
            .transpose()
    }

    pub fn list_model_invocations(&self, run_id: &str) -> Result<Vec<ModelInvocationRecord>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT record_json FROM model_invocations WHERE run_id = ?1 ORDER BY started_at",
        )?;
        let rows = statement.query_map(params![run_id], |row| row.get::<_, String>(0))?;
        let mut invocations = Vec::new();
        for row in rows {
            let payload = row?;
            if let Ok(invocation) = serde_json::from_str::<ModelInvocationRecord>(&payload) {
                invocations.push(invocation);
            }
        }
        Ok(invocations)
    }

    pub fn reconcile_interrupted_model_invocations(&self) -> Result<usize> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT record_json FROM model_invocations WHERE status IN ('Queued', 'Running')",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut interrupted = Vec::new();
        for payload in rows {
            let payload = payload?;
            if let Ok(mut invocation) = serde_json::from_str::<ModelInvocationRecord>(&payload) {
                invocation.status = InvocationStatus::Failed;
                invocation.finished_at_epoch_millis = Some(now_millis());
                invocation.duration_millis = invocation
                    .finished_at_epoch_millis
                    .unwrap_or_default()
                    .saturating_sub(invocation.started_at_epoch_millis);
                invocation.failure_class = Some(FailureClass::Internal);
                invocation.error_code = Some(ErrorCode::new("runtime_interrupted"));
                interrupted.push(invocation);
            }
        }
        drop(statement);
        for invocation in &interrupted {
            self.save_model_invocation(invocation)?;
        }
        Ok(interrupted.len())
    }

    pub fn reconcile_interrupted_tool_invocations(&self) -> Result<usize> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT record_json FROM tool_invocations WHERE status IN ('Queued', 'Running')",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut interrupted = Vec::new();
        for payload in rows {
            let payload = payload?;
            if let Ok(mut invocation) = serde_json::from_str::<ToolInvocationRecord>(&payload) {
                invocation.status = InvocationStatus::Failed;
                invocation.finished_at_epoch_millis = Some(now_millis());
                invocation.result_summary =
                    Some("runtime stopped before invocation completed".to_owned());
                invocation.failure_class = Some(FailureClass::Internal);
                invocation.error_code = Some(ErrorCode::new("runtime_interrupted"));
                interrupted.push(invocation);
            }
        }
        drop(statement);
        for invocation in &interrupted {
            self.save_tool_invocation(invocation)?;
        }
        Ok(interrupted.len())
    }

    pub fn save_tool_invocation(&self, invocation: &ToolInvocationRecord) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO tool_invocations (
                invocation_id, run_id, tool_name, status, started_at, finished_at, replay_key, record_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(invocation_id) DO UPDATE SET
                status = excluded.status,
                finished_at = excluded.finished_at,
                replay_key = excluded.replay_key,
                record_json = excluded.record_json",
            params![
                invocation.invocation_id.as_str(),
                invocation.run_id.as_str(),
                invocation.tool_name,
                format!("{:?}", invocation.status),
                millis_as_i64(invocation.started_at_epoch_millis),
                invocation.finished_at_epoch_millis.map(millis_as_i64),
                invocation.replay_key.as_deref(),
                serde_json::to_string(invocation)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_tool_invocation(&self, invocation_id: &str) -> Result<Option<ToolInvocationRecord>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT record_json FROM tool_invocations WHERE invocation_id = ?1",
                params![invocation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
            .transpose()
    }

    pub fn latest_failed_run_invocation_by_replay_key(
        &self,
        replay_key: &str,
    ) -> Result<Option<ToolInvocationRecord>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT invocation.record_json \
                 FROM tool_invocations invocation \
                 JOIN runs run ON run.run_id = invocation.run_id \
                 WHERE invocation.replay_key = ?1 \
                   AND invocation.tool_name = 'workspace.apply_patch' \
                   AND run.status = 'Failed' \
                 ORDER BY invocation.finished_at DESC, invocation.started_at DESC \
                 LIMIT 1",
                params![replay_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
            .transpose()
    }

    pub fn list_tool_invocations(&self, run_id: &str) -> Result<Vec<ToolInvocationRecord>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT record_json FROM tool_invocations WHERE run_id = ?1 ORDER BY started_at, invocation_id",
        )?;
        let rows = statement.query_map(params![run_id], |row| row.get::<_, String>(0))?;
        let mut invocations = Vec::new();
        for payload in rows {
            if let Ok(invocation) = serde_json::from_str::<ToolInvocationRecord>(&payload?) {
                invocations.push(invocation);
            }
        }
        Ok(invocations)
    }

    pub fn save_artifact(&self, artifact: &ArtifactDescriptor) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO artifacts ( \
                artifact_id, run_id, content_hash, kind, media_type, size_bytes, \
                object_path, created_at, origin, descriptor_json \
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(artifact_id) DO UPDATE SET \
                descriptor_json = excluded.descriptor_json",
            params![
                artifact.artifact_id,
                artifact.run_id,
                artifact.content_hash,
                format!("{:?}", artifact.kind),
                artifact.media_type,
                i64::try_from(artifact.size_bytes).unwrap_or(i64::MAX),
                artifact.object_path.display().to_string(),
                millis_as_i64(artifact.created_at_epoch_millis),
                artifact.origin,
                serde_json::to_string(artifact)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_artifact(&self, artifact_id: &str) -> Result<Option<ArtifactDescriptor>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT descriptor_json FROM artifacts WHERE artifact_id = ?1",
                params![artifact_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| {
                serde_json::from_str(&payload).with_context(|| {
                    format!("state record for artifact '{artifact_id}' is corrupt")
                })
            })
            .transpose()
    }

    pub fn list_artifacts(&self, run_id: &str) -> Result<Vec<ArtifactDescriptor>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT descriptor_json FROM artifacts WHERE run_id = ?1 ORDER BY created_at",
        )?;
        let rows = statement.query_map(params![run_id], |row| row.get::<_, String>(0))?;
        let mut artifacts = Vec::new();
        for row in rows {
            let payload = row?;
            if let Ok(artifact) = serde_json::from_str::<ArtifactDescriptor>(&payload) {
                artifacts.push(artifact);
            }
        }
        Ok(artifacts)
    }

    pub fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO checkpoints ( \
                checkpoint_id, run_id, event_sequence, safe_to_resume, created_at, checkpoint_json \
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(checkpoint_id) DO UPDATE SET \
                safe_to_resume = excluded.safe_to_resume, \
                checkpoint_json = excluded.checkpoint_json",
            params![
                checkpoint.checkpoint_id.as_str(),
                checkpoint.run_id.as_str(),
                i64::try_from(checkpoint.event_sequence).unwrap_or(i64::MAX),
                if checkpoint.safe_to_resume {
                    1_i64
                } else {
                    0_i64
                },
                millis_as_i64(checkpoint.created_at_epoch_millis),
                serde_json::to_string(checkpoint)?,
            ],
        )?;
        Ok(())
    }

    pub fn latest_safe_checkpoint(&self, run_id: &str) -> Result<Option<Checkpoint>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT checkpoint_json FROM checkpoints \
                 WHERE run_id = ?1 AND safe_to_resume = 1 \
                 ORDER BY event_sequence DESC, created_at DESC LIMIT 1",
                params![run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        payload
            .map(|payload| {
                serde_json::from_str(&payload).with_context(|| {
                    format!("safe checkpoint record for run '{run_id}' is corrupt")
                })
            })
            .transpose()
    }

    pub fn list_checkpoints(&self, run_id: &str) -> Result<Vec<Checkpoint>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT checkpoint_json FROM checkpoints WHERE run_id = ?1 \
             ORDER BY event_sequence, created_at",
        )?;
        let rows = statement.query_map(params![run_id], |row| row.get::<_, String>(0))?;
        let mut checkpoints = Vec::new();
        for row in rows {
            let payload = row?;
            if let Ok(checkpoint) = serde_json::from_str::<Checkpoint>(&payload) {
                checkpoints.push(checkpoint);
            }
        }
        Ok(checkpoints)
    }

    pub fn save_automation(&self, automation: &AutomationDefinition) -> Result<()> {
        automation.schedule.validate().map_err(anyhow::Error::msg)?;
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO automations (
                automation_id, enabled, next_run_at, updated_at, definition_json
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(automation_id) DO UPDATE SET
                enabled = excluded.enabled,
                next_run_at = excluded.next_run_at,
                updated_at = excluded.updated_at,
                definition_json = excluded.definition_json",
            params![
                automation.automation_id,
                if automation.enabled { 1i64 } else { 0i64 },
                automation.next_run_at_epoch_millis.map(millis_as_i64),
                millis_as_i64(automation.updated_at_epoch_millis),
                serde_json::to_string(automation)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_automation(&self, automation_id: &str) -> Result<Option<AutomationDefinition>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT definition_json FROM automations WHERE automation_id = ?1",
                params![automation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| {
                serde_json::from_str(&payload)
                    .with_context(|| format!("automation '{automation_id}' is corrupt"))
            })
            .transpose()
    }

    pub fn list_automations(&self) -> Result<Vec<AutomationDefinition>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT definition_json FROM automations ORDER BY enabled DESC, next_run_at, updated_at DESC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut automations = Vec::new();
        for payload in rows {
            if let Ok(automation) = serde_json::from_str::<AutomationDefinition>(&payload?) {
                automations.push(automation);
            }
        }
        Ok(automations)
    }

    pub fn delete_automation(&self, automation_id: &str) -> Result<bool> {
        let connection = self.open()?;
        Ok(connection.execute(
            "DELETE FROM automations WHERE automation_id = ?1",
            params![automation_id],
        )? > 0)
    }

    pub fn claim_due_automation(
        &self,
        now_epoch_millis: u128,
        minimum_lease_millis: u64,
    ) -> Result<Option<AutomationLease>> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        let now = millis_as_i64(now_epoch_millis);
        let payload = transaction
            .query_row(
                "SELECT automation_id, definition_json
                 FROM automations
                 WHERE enabled = 1
                   AND next_run_at IS NOT NULL
                   AND next_run_at <= ?1
                   AND (lease_until IS NULL OR lease_until <= ?1)
                 ORDER BY next_run_at, updated_at
                 LIMIT 1",
                params![now],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((automation_id, payload)) = payload else {
            transaction.commit()?;
            return Ok(None);
        };
        let automation: AutomationDefinition = serde_json::from_str(&payload)
            .with_context(|| format!("automation '{automation_id}' is corrupt"))?;
        let lease_millis = automation_lease_millis(&automation, minimum_lease_millis);
        let lease_until_epoch_millis = now_epoch_millis.saturating_add(u128::from(lease_millis));
        let lease_owner = generated_lease_owner(&automation_id);
        let changed = transaction.execute(
            "UPDATE automations
             SET lease_until = ?2, lease_owner = ?3
             WHERE automation_id = ?1
               AND (lease_until IS NULL OR lease_until <= ?4)",
            params![
                automation_id,
                millis_as_i64(lease_until_epoch_millis),
                lease_owner,
                now,
            ],
        )?;
        if changed == 0 {
            transaction.commit()?;
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(AutomationLease {
            automation,
            lease_owner,
            lease_until_epoch_millis,
        }))
    }

    pub fn acquire_automation_lease(
        &self,
        automation_id: &str,
        now_epoch_millis: u128,
        lease_millis: u64,
    ) -> Result<Option<String>> {
        let connection = self.open()?;
        let now = millis_as_i64(now_epoch_millis);
        let lease_owner = generated_lease_owner(automation_id);
        let changed = connection.execute(
            "UPDATE automations
             SET lease_until = ?2, lease_owner = ?3
             WHERE automation_id = ?1
               AND (lease_until IS NULL OR lease_until <= ?4)",
            params![
                automation_id,
                millis_as_i64(now_epoch_millis.saturating_add(u128::from(lease_millis))),
                lease_owner,
                now,
            ],
        )?;
        Ok((changed > 0).then_some(lease_owner))
    }

    pub fn renew_automation_lease(
        &self,
        automation_id: &str,
        lease_owner: &str,
        now_epoch_millis: u128,
        lease_millis: u64,
    ) -> Result<bool> {
        let connection = self.open()?;
        let changed = connection.execute(
            "UPDATE automations
             SET lease_until = ?3
             WHERE automation_id = ?1 AND lease_owner = ?2",
            params![
                automation_id,
                lease_owner,
                millis_as_i64(now_epoch_millis.saturating_add(u128::from(lease_millis))),
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn release_automation_lease(&self, automation_id: &str, lease_owner: &str) -> Result<bool> {
        let connection = self.open()?;
        let changed = connection.execute(
            "UPDATE automations
             SET lease_until = NULL, lease_owner = NULL
             WHERE automation_id = ?1 AND lease_owner = ?2",
            params![automation_id, lease_owner],
        )?;
        Ok(changed > 0)
    }

    pub fn save_automation_execution(&self, execution: &AutomationExecution) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO automation_executions (
                execution_id, automation_id, status, scheduled_for, started_at, finished_at, execution_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(execution_id) DO UPDATE SET
                status = excluded.status,
                finished_at = excluded.finished_at,
                execution_json = excluded.execution_json",
            params![
                execution.execution_id,
                execution.automation_id,
                format!("{:?}", execution.status),
                millis_as_i64(execution.scheduled_for_epoch_millis),
                millis_as_i64(execution.started_at_epoch_millis),
                execution.finished_at_epoch_millis.map(millis_as_i64),
                serde_json::to_string(execution)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_automation_execution(
        &self,
        execution_id: &str,
    ) -> Result<Option<AutomationExecution>> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT execution_json FROM automation_executions WHERE execution_id = ?1",
                [execution_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|value| {
                serde_json::from_str::<AutomationExecution>(&value)
                    .context("stored automation execution is corrupt")
            })
            .transpose()
    }

    pub fn list_automation_executions(
        &self,
        automation_id: &str,
        limit: usize,
    ) -> Result<Vec<AutomationExecution>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT execution_json FROM automation_executions
             WHERE automation_id = ?1
             ORDER BY started_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![
                automation_id,
                i64::try_from(limit.clamp(1, 500)).unwrap_or(500)
            ],
            |row| row.get::<_, String>(0),
        )?;
        let mut executions = Vec::new();
        for payload in rows {
            if let Ok(execution) = serde_json::from_str::<AutomationExecution>(&payload?) {
                executions.push(execution);
            }
        }
        Ok(executions)
    }

    pub fn count_automation_executions_since(
        &self,
        automation_id: &str,
        since_epoch_millis: u128,
    ) -> Result<u32> {
        let connection = self.open()?;
        let count = connection.query_row(
            "SELECT COUNT(*) FROM automation_executions
             WHERE automation_id = ?1
               AND started_at >= ?2
               AND status IN ('Running', 'Completed', 'Failed', 'DeadLetter')",
            params![automation_id, millis_as_i64(since_epoch_millis)],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    pub fn recoverable_journals(&self) -> Result<Vec<RunJournal>> {
        Ok(self
            .load_journals()?
            .entries
            .into_iter()
            .map(|(journal, _)| journal)
            .filter(|journal| journal.status.is_recoverable())
            .collect())
    }

    fn initialize(&self) -> Result<()> {
        if let Some(parent) = self.database_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = self.open()?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS state_metadata ( \
                key TEXT PRIMARY KEY, \
                value TEXT NOT NULL \
             );",
        )?;

        let existing_version = connection
            .query_row(
                "SELECT value FROM state_metadata WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        if existing_version > STATE_SCHEMA_VERSION {
            anyhow::bail!(
                "state database schema {existing_version} is newer than supported schema {STATE_SCHEMA_VERSION}"
            );
        }

        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS runs ( \
                run_id TEXT PRIMARY KEY, \
                objective TEXT NOT NULL, \
                status TEXT NOT NULL, \
                generated_by TEXT NOT NULL, \
                created_at INTEGER NOT NULL, \
                updated_at INTEGER NOT NULL, \
                journal_json TEXT NOT NULL \
             ); \
             CREATE TABLE IF NOT EXISTS run_events ( \
                run_id TEXT NOT NULL, \
                sequence INTEGER NOT NULL, \
                event_id TEXT NOT NULL, \
                timestamp INTEGER NOT NULL, \
                event_kind TEXT NOT NULL, \
                stage TEXT NOT NULL, \
                event_json TEXT NOT NULL, \
                PRIMARY KEY (run_id, sequence), \
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE \
             ); \
             CREATE TABLE IF NOT EXISTS artifacts ( \
                artifact_id TEXT PRIMARY KEY, \
                run_id TEXT NOT NULL, \
                content_hash TEXT NOT NULL, \
                kind TEXT NOT NULL, \
                media_type TEXT NOT NULL, \
                size_bytes INTEGER NOT NULL, \
                object_path TEXT NOT NULL, \
                created_at INTEGER NOT NULL, \
                origin TEXT NOT NULL, \
                descriptor_json TEXT NOT NULL, \
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE \
             ); \
             CREATE TABLE IF NOT EXISTS model_invocations ( \
                invocation_id TEXT PRIMARY KEY, \
                run_id TEXT NOT NULL, \
                provider TEXT NOT NULL, \
                model_name TEXT NOT NULL, \
                status TEXT NOT NULL, \
                started_at INTEGER NOT NULL, \
                finished_at INTEGER, \
                record_json TEXT NOT NULL, \
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE \
             ); \
             CREATE TABLE IF NOT EXISTS tool_invocations ( \
                invocation_id TEXT PRIMARY KEY, \
                run_id TEXT NOT NULL, \
                tool_name TEXT NOT NULL, \
                status TEXT NOT NULL, \
                started_at INTEGER NOT NULL, \
                finished_at INTEGER, \
                replay_key TEXT, \
                record_json TEXT NOT NULL, \
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE \
             ); \
             CREATE TABLE IF NOT EXISTS checkpoints ( \
                checkpoint_id TEXT PRIMARY KEY, \
                run_id TEXT NOT NULL, \
                event_sequence INTEGER NOT NULL, \
                safe_to_resume INTEGER NOT NULL, \
                created_at INTEGER NOT NULL, \
                checkpoint_json TEXT NOT NULL, \
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE \
             ); \
             CREATE TABLE IF NOT EXISTS automations ( \
                automation_id TEXT PRIMARY KEY, \
                enabled INTEGER NOT NULL, \
                next_run_at INTEGER, \
                updated_at INTEGER NOT NULL, \
                lease_until INTEGER, \
                lease_owner TEXT, \
                definition_json TEXT NOT NULL \
             ); \
             CREATE TABLE IF NOT EXISTS automation_executions ( \
                execution_id TEXT PRIMARY KEY, \
                automation_id TEXT NOT NULL, \
                status TEXT NOT NULL, \
                scheduled_for INTEGER NOT NULL, \
                started_at INTEGER NOT NULL, \
                finished_at INTEGER, \
                execution_json TEXT NOT NULL, \
                FOREIGN KEY (automation_id) REFERENCES automations(automation_id) ON DELETE CASCADE \
             ); \
             CREATE INDEX IF NOT EXISTS idx_runs_updated_at ON runs(updated_at DESC); \
             CREATE INDEX IF NOT EXISTS idx_run_events_timestamp ON run_events(timestamp); \
             CREATE INDEX IF NOT EXISTS idx_artifacts_run_id ON artifacts(run_id, created_at); \
             CREATE INDEX IF NOT EXISTS idx_model_invocations_run_id \
                ON model_invocations(run_id, started_at); \
             CREATE INDEX IF NOT EXISTS idx_tool_invocations_run_id \
                ON tool_invocations(run_id, started_at); \
             CREATE INDEX IF NOT EXISTS idx_checkpoints_run_id \
                ON checkpoints(run_id, safe_to_resume, event_sequence DESC); \
             CREATE INDEX IF NOT EXISTS idx_automations_due \
                ON automations(enabled, next_run_at, lease_until); \
             CREATE INDEX IF NOT EXISTS idx_automation_executions_history \
                ON automation_executions(automation_id, started_at DESC);",
        )?;
        if !table_has_column(&connection, "automations", "lease_owner")? {
            connection.execute("ALTER TABLE automations ADD COLUMN lease_owner TEXT", [])?;
        }
        if !table_has_column(&connection, "tool_invocations", "replay_key")? {
            connection.execute(
                "ALTER TABLE tool_invocations ADD COLUMN replay_key TEXT",
                [],
            )?;
        }
        connection.execute(
            "CREATE INDEX IF NOT EXISTS idx_tool_invocations_replay_key \
             ON tool_invocations(replay_key, status, finished_at DESC)",
            [],
        )?;

        connection.execute(
            "INSERT INTO state_metadata (key, value) VALUES ('schema_version', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![STATE_SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        let connection = Connection::open(&self.database_path)
            .with_context(|| format!("open state database {}", self.database_path.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn automation_lease_millis(automation: &AutomationDefinition, minimum: u64) -> u64 {
    minimum.max(
        automation
            .policy
            .budget
            .max_runtime_millis
            .saturating_add(60_000),
    )
}

fn generated_lease_owner(automation_id: &str) -> String {
    let sequence = AUTOMATION_LEASE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}:{}:{}:{}",
        automation_id,
        std::process::id(),
        now_millis(),
        sequence
    )
}

fn millis_as_i64(value: u128) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::StateStore;
    use everything_domain::{
        ArtifactDescriptor, ArtifactKind, AutomationAction, AutomationDefinition,
        AutomationExecution, AutomationExecutionStatus, AutomationPolicy, AutomationSchedule,
        Checkpoint, CheckpointId, ExecutionMode, FailureClass, InvocationId, InvocationStatus,
        PermissionScope, RunEvent, RunId, RunJournal, RunStatus, ToolEffect, ToolInvocationRecord,
    };
    use rusqlite::params;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(label: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("everything-state-{label}-{suffix}"))
    }

    fn journal(run_id: &str) -> RunJournal {
        RunJournal {
            schema_version: 2,
            run_id: run_id.to_owned(),
            objective: "test state".to_owned(),
            status: RunStatus::Running,
            generated_by: "test-model".to_owned(),
            mode: None,
            created_at_epoch_millis: 1,
            updated_at_epoch_millis: 2,
            events: vec![RunEvent {
                sequence: 0,
                event_id: format!("{run_id}-event-0"),
                timestamp_epoch_millis: 2,
                stage: "run".to_owned(),
                stage_id: None,
                correlation_id: Some(run_id.to_owned()),
                event_kind: "run.started".to_owned(),
                severity: Default::default(),
                summary: "Run started".to_owned(),
                detail: "status=running".to_owned(),
                payload: serde_json::Value::Null,
                provenance: "test".to_owned(),
            }],
            artifact_path: None,
            artifacts: Vec::new(),
            failure_class: None,
            recoverable: true,
            recovery_disposition: None,
        }
    }

    fn automation(automation_id: &str) -> AutomationDefinition {
        AutomationDefinition {
            automation_id: automation_id.to_owned(),
            name: "test automation".to_owned(),
            description: String::new(),
            schedule: AutomationSchedule::Interval {
                every_millis: 60_000,
                anchor_epoch_millis: Some(1),
            },
            action: AutomationAction::Plan {
                objective: "inspect".to_owned(),
                mode: ExecutionMode::Fast,
            },
            policy: AutomationPolicy::default(),
            enabled: true,
            created_at_epoch_millis: 1,
            updated_at_epoch_millis: 1,
            next_run_at_epoch_millis: Some(1),
            last_run_at_epoch_millis: None,
            consecutive_failures: 0,
            retry_attempt: 0,
            retry_scheduled_for_epoch_millis: None,
            suspended_reason: None,
        }
    }

    #[test]
    fn automation_lease_is_exclusive_until_expiry() {
        let root = test_root("automation-lease");
        let store = StateStore::new(root.join("runtime.sqlite3")).expect("store");
        store
            .save_automation(&automation("auto-1"))
            .expect("save automation");
        let first = store
            .acquire_automation_lease("auto-1", 10, 1_000)
            .expect("first lease")
            .expect("lease owner");
        assert!(
            store
                .acquire_automation_lease("auto-1", 11, 1_000)
                .expect("second lease")
                .is_none()
        );
        let second = store
            .acquire_automation_lease("auto-1", 1_011, 1_000)
            .expect("expired lease")
            .expect("replacement owner");
        assert_ne!(first, second);
        assert!(
            !store
                .release_automation_lease("auto-1", &first)
                .expect("stale release")
        );
        assert!(
            store
                .renew_automation_lease("auto-1", &second, 1_012, 1_000)
                .expect("renew current lease")
        );
        assert!(
            store
                .release_automation_lease("auto-1", &second)
                .expect("release")
        );
        assert!(
            store
                .acquire_automation_lease("auto-1", 12, 1_000)
                .expect("lease after release")
                .is_some()
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn daily_budget_counts_only_billable_execution_states() {
        let root = test_root("automation-budget");
        let store = StateStore::new(root.join("runtime.sqlite3")).expect("store");
        store
            .save_automation(&automation("auto-budget"))
            .expect("save automation");
        for (index, status) in [
            AutomationExecutionStatus::AwaitingApproval,
            AutomationExecutionStatus::Approved,
            AutomationExecutionStatus::Skipped,
            AutomationExecutionStatus::Running,
            AutomationExecutionStatus::Completed,
            AutomationExecutionStatus::Failed,
            AutomationExecutionStatus::DeadLetter,
        ]
        .into_iter()
        .enumerate()
        {
            store
                .save_automation_execution(&AutomationExecution {
                    execution_id: format!("execution-{index}"),
                    automation_id: "auto-budget".to_owned(),
                    status,
                    scheduled_for_epoch_millis: 1_000,
                    started_at_epoch_millis: 1_000 + u128::try_from(index).expect("index"),
                    finished_at_epoch_millis: Some(2_000),
                    run_id: None,
                    output: serde_json::Value::Null,
                    error: None,
                    attempt: 1,
                    next_retry_at_epoch_millis: None,
                })
                .expect("save execution");
        }

        assert_eq!(
            store
                .count_automation_executions_since("auto-budget", 0)
                .expect("count"),
            4
        );

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn journals_round_trip_through_sqlite() {
        let root = test_root("roundtrip");
        let store = StateStore::new(root.join("runtime.sqlite3")).expect("store");
        store.save_journal(&journal("run-1")).expect("save");

        let loaded = store.get_journal("run-1").expect("load").expect("journal");
        assert_eq!(loaded.run_id, "run-1");
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(store.recoverable_journals().expect("recoverable").len(), 1);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn corrupt_record_does_not_hide_other_runs() {
        let root = test_root("corrupt");
        let database = root.join("runtime.sqlite3");
        let store = StateStore::new(&database).expect("store");
        store.save_journal(&journal("run-good")).expect("save good");
        store.save_journal(&journal("run-bad")).expect("save bad");

        let connection = rusqlite::Connection::open(&database).expect("open database");
        connection
            .execute(
                "UPDATE runs SET journal_json = ?1 WHERE run_id = ?2",
                params!["{not-json", "run-bad"],
            )
            .expect("corrupt row");

        let report = store.load_journals().expect("load report");
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].0.run_id, "run-good");
        assert_eq!(report.corrupt_run_ids, vec!["run-bad"]);

        drop(connection);
        drop(store);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn corrupt_event_is_isolated_from_valid_events() {
        let root = test_root("corrupt-event");
        let database = root.join("runtime.sqlite3");
        let store = StateStore::new(&database).expect("store");
        let mut payload = journal("run-events");
        let mut second = payload.events[0].clone();
        second.sequence = 1;
        second.event_id = "run-events-event-1".to_owned();
        second.detail = "second".to_owned();
        payload.events.push(second);
        store.save_journal(&payload).expect("save run");

        let connection = rusqlite::Connection::open(&database).expect("open database");
        connection
            .execute(
                "UPDATE run_events SET event_json = ?1 WHERE run_id = ?2 AND sequence = 0",
                params!["{not-json", "run-events"],
            )
            .expect("corrupt event");

        let events = store.load_events("run-events").expect("load events");
        assert_eq!(events.entries.len(), 1);
        assert_eq!(events.entries[0].detail, "second");
        assert_eq!(events.corrupt_sequences, vec![0]);
        let loaded = store
            .get_journal("run-events")
            .expect("load run")
            .expect("journal");
        assert_eq!(loaded.events.len(), 1);

        drop(connection);
        drop(store);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn latest_safe_checkpoint_ignores_unsafe_boundaries() {
        let root = test_root("checkpoints");
        let store = StateStore::new(root.join("runtime.sqlite3")).expect("store");
        store.save_journal(&journal("run-1")).expect("save run");
        for (id, sequence, safe) in [("cp-1", 1, true), ("cp-2", 2, false)] {
            store
                .save_checkpoint(&Checkpoint {
                    checkpoint_id: CheckpointId::from(id),
                    run_id: RunId::from("run-1"),
                    stage_id: None,
                    event_sequence: sequence,
                    created_at_epoch_millis: u128::from(sequence),
                    safe_to_resume: safe,
                    summary: id.to_owned(),
                    artifact_ids: Vec::new(),
                })
                .expect("save checkpoint");
        }

        let checkpoint = store
            .latest_safe_checkpoint("run-1")
            .expect("load checkpoint")
            .expect("safe checkpoint");
        assert_eq!(checkpoint.checkpoint_id.as_str(), "cp-1");

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn replay_lookup_finds_patch_from_failed_run() {
        let root = test_root("replay-lookup");
        let store = StateStore::new(root.join("runtime.sqlite3")).expect("store");
        let mut failed_run = journal("run-failed");
        failed_run.status = RunStatus::Failed;
        failed_run.recoverable = false;
        failed_run.failure_class = Some(FailureClass::Validation);
        store.save_journal(&failed_run).expect("save failed run");
        store
            .save_tool_invocation(&ToolInvocationRecord {
                invocation_id: InvocationId::from("inv-patch"),
                run_id: RunId::from("run-failed"),
                stage_id: None,
                tool_name: "workspace.apply_patch".to_owned(),
                tool_version: "1.0.0".to_owned(),
                required_permissions: vec![PermissionScope::WorkspaceWrite],
                effect: ToolEffect::WorkspaceMutation,
                status: InvocationStatus::Completed,
                started_at_epoch_millis: 1,
                finished_at_epoch_millis: Some(2),
                arguments: serde_json::json!({"path": "demo.txt"}),
                output: serde_json::json!({"diff": "example"}),
                output_truncated: false,
                timeout_millis: Some(1_000),
                replay_key: Some("patch-replay".to_owned()),
                result_summary: Some("patch applied".to_owned()),
                failure_class: None,
                error_code: None,
            })
            .expect("save patch invocation");

        let invocation = store
            .latest_failed_run_invocation_by_replay_key("patch-replay")
            .expect("lookup")
            .expect("invocation");
        assert_eq!(invocation.invocation_id.as_str(), "inv-patch");
        assert_eq!(invocation.status, InvocationStatus::Completed);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn interrupted_tool_invocations_are_marked_failed_on_recovery() {
        let root = test_root("tool-recovery");
        let store = StateStore::new(root.join("runtime.sqlite3")).expect("store");
        store.save_journal(&journal("run-1")).expect("save run");
        store
            .save_tool_invocation(&ToolInvocationRecord {
                invocation_id: InvocationId::from("inv-running"),
                run_id: RunId::from("run-1"),
                stage_id: None,
                tool_name: "process.run".to_owned(),
                tool_version: "1.0.0".to_owned(),
                required_permissions: vec![PermissionScope::ProcessExecute],
                effect: ToolEffect::Process,
                status: InvocationStatus::Running,
                started_at_epoch_millis: 1,
                finished_at_epoch_millis: None,
                arguments: serde_json::json!({"program": "cargo"}),
                output: serde_json::Value::Null,
                output_truncated: false,
                timeout_millis: Some(1_000),
                replay_key: Some("replay".to_owned()),
                result_summary: Some("running".to_owned()),
                failure_class: None,
                error_code: None,
            })
            .expect("save running invocation");

        assert_eq!(
            store
                .reconcile_interrupted_tool_invocations()
                .expect("reconcile"),
            1
        );
        let recovered = store
            .get_tool_invocation("inv-running")
            .expect("load")
            .expect("invocation");
        assert_eq!(recovered.status, InvocationStatus::Failed);
        assert_eq!(recovered.failure_class, Some(FailureClass::Internal));
        assert_eq!(
            recovered.error_code.as_ref().map(|code| code.as_str()),
            Some("runtime_interrupted")
        );

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn tool_invocations_round_trip_and_corrupt_rows_are_isolated() {
        let root = test_root("tool-invocations");
        let database = root.join("runtime.sqlite3");
        let store = StateStore::new(&database).expect("store");
        store.save_journal(&journal("run-1")).expect("save run");
        for invocation_id in ["inv-good", "inv-bad"] {
            store
                .save_tool_invocation(&ToolInvocationRecord {
                    invocation_id: InvocationId::from(invocation_id),
                    run_id: RunId::from("run-1"),
                    stage_id: None,
                    tool_name: "workspace.read_file".to_owned(),
                    tool_version: "1.0.0".to_owned(),
                    required_permissions: vec![PermissionScope::WorkspaceRead],
                    effect: ToolEffect::ReadOnly,
                    status: InvocationStatus::Completed,
                    started_at_epoch_millis: 1,
                    finished_at_epoch_millis: Some(2),
                    arguments: serde_json::json!({"path": "README.md"}),
                    output: serde_json::json!({"content": "hello"}),
                    output_truncated: false,
                    timeout_millis: Some(1_000),
                    replay_key: Some("replay".to_owned()),
                    result_summary: Some("completed".to_owned()),
                    failure_class: None,
                    error_code: None,
                })
                .expect("save invocation");
        }

        let loaded = store
            .get_tool_invocation("inv-good")
            .expect("load invocation")
            .expect("invocation");
        assert_eq!(loaded.tool_version, "1.0.0");
        assert_eq!(loaded.failure_class, None::<FailureClass>);

        let connection = rusqlite::Connection::open(&database).expect("open database");
        connection
            .execute(
                "UPDATE tool_invocations SET record_json = ?1 WHERE invocation_id = ?2",
                params!["{not-json", "inv-bad"],
            )
            .expect("corrupt invocation");
        let invocations = store
            .list_tool_invocations("run-1")
            .expect("list invocations");
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].invocation_id.as_str(), "inv-good");

        drop(connection);
        drop(store);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn corrupt_artifact_descriptor_is_skipped() {
        let root = test_root("corrupt-artifact");
        let database = root.join("runtime.sqlite3");
        let store = StateStore::new(&database).expect("store");
        store.save_journal(&journal("run-1")).expect("save run");
        for artifact_id in ["artifact-good", "artifact-bad"] {
            store
                .save_artifact(&ArtifactDescriptor {
                    artifact_id: artifact_id.to_owned(),
                    run_id: "run-1".to_owned(),
                    kind: ArtifactKind::Plan,
                    content_hash: artifact_id.to_owned(),
                    media_type: "text/plain".to_owned(),
                    size_bytes: 1,
                    object_path: root.join(artifact_id),
                    created_at_epoch_millis: 1,
                    origin: "test".to_owned(),
                })
                .expect("save artifact");
        }

        let connection = rusqlite::Connection::open(&database).expect("open database");
        connection
            .execute(
                "UPDATE artifacts SET descriptor_json = ?1 WHERE artifact_id = ?2",
                params!["{not-json", "artifact-bad"],
            )
            .expect("corrupt artifact");

        let artifacts = store.list_artifacts("run-1").expect("list artifacts");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_id, "artifact-good");

        drop(connection);
        drop(store);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
