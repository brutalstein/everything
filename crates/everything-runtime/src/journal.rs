use everything_domain::{
    ArtifactDescriptor, EventSeverity, ExecutionMode, FailureClass, RUN_JOURNAL_SCHEMA_VERSION,
    RecoveryDisposition, RunEvent, RunJournal, RunStatus,
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct RunJournalBuilder {
    run_id: String,
    objective: String,
    generated_by: String,
    mode: Option<ExecutionMode>,
    created_at_epoch_millis: u128,
    next_event_sequence: u64,
    events: Vec<RunEvent>,
    artifacts: Vec<ArtifactDescriptor>,
    failure_class: Option<FailureClass>,
}

impl RunJournalBuilder {
    #[cfg(test)]
    pub fn new(objective: impl Into<String>, generated_by: impl Into<String>) -> Self {
        Self::new_with_mode(objective, generated_by, None)
    }

    pub fn new_for_task(
        objective: impl Into<String>,
        generated_by: impl Into<String>,
        mode: ExecutionMode,
    ) -> Self {
        Self::new_with_mode(objective, generated_by, Some(mode))
    }

    fn new_with_mode(
        objective: impl Into<String>,
        generated_by: impl Into<String>,
        mode: Option<ExecutionMode>,
    ) -> Self {
        let timestamp = now_epoch_millis();
        let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);

        Self {
            run_id: format!("run-{timestamp}-{}-{sequence}", std::process::id()),
            objective: objective.into(),
            generated_by: generated_by.into(),
            mode,
            created_at_epoch_millis: timestamp,
            next_event_sequence: 0,
            events: Vec::new(),
            artifacts: Vec::new(),
            failure_class: None,
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn last_event_sequence(&self) -> Option<u64> {
        self.events.last().map(|event| event.sequence)
    }

    pub fn set_generated_by(&mut self, generated_by: impl Into<String>) {
        self.generated_by = generated_by.into();
    }

    pub fn set_failure_class(&mut self, failure_class: FailureClass) {
        self.failure_class = Some(failure_class);
    }

    pub fn add_artifact(&mut self, artifact: ArtifactDescriptor) {
        self.artifacts.push(artifact);
    }

    pub fn push(&mut self, stage: impl Into<String>, detail: impl Into<String>) -> RunEvent {
        self.push_structured(
            stage,
            "runtime.event",
            EventSeverity::Info,
            "Runtime event",
            detail,
            Value::Null,
            "everything-runtime",
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_structured(
        &mut self,
        stage: impl Into<String>,
        event_kind: impl Into<String>,
        severity: EventSeverity,
        summary: impl Into<String>,
        detail: impl Into<String>,
        payload: Value,
        provenance: impl Into<String>,
    ) -> RunEvent {
        let sequence = self.next_event_sequence;
        self.next_event_sequence += 1;
        let stage = stage.into();
        let event = RunEvent {
            sequence,
            event_id: format!("{}-event-{sequence}", self.run_id),
            timestamp_epoch_millis: now_epoch_millis(),
            stage_id: Some(format!("{}:{stage}", self.run_id)),
            correlation_id: Some(self.run_id.clone()),
            event_kind: event_kind.into(),
            severity,
            summary: summary.into(),
            stage,
            detail: detail.into(),
            payload,
            provenance: provenance.into(),
        };
        self.events.push(event.clone());
        event
    }

    pub fn build(self, status: RunStatus, artifact_path: Option<PathBuf>) -> RunJournal {
        let updated_at_epoch_millis = now_epoch_millis();
        RunJournal {
            schema_version: RUN_JOURNAL_SCHEMA_VERSION,
            run_id: self.run_id,
            objective: self.objective,
            status,
            generated_by: self.generated_by,
            mode: self.mode,
            created_at_epoch_millis: self.created_at_epoch_millis,
            updated_at_epoch_millis,
            events: self.events,
            artifact_path,
            artifacts: self.artifacts,
            failure_class: self.failure_class,
            recoverable: status.is_recoverable(),
            recovery_disposition: if status.is_recoverable() {
                Some(RecoveryDisposition::Recoverable)
            } else {
                Some(RecoveryDisposition::NotRecoverable)
            },
        }
    }

    pub fn snapshot(&self, status: RunStatus, artifact_path: Option<PathBuf>) -> RunJournal {
        RunJournal {
            schema_version: RUN_JOURNAL_SCHEMA_VERSION,
            run_id: self.run_id.clone(),
            objective: self.objective.clone(),
            status,
            generated_by: self.generated_by.clone(),
            mode: self.mode,
            created_at_epoch_millis: self.created_at_epoch_millis,
            updated_at_epoch_millis: now_epoch_millis(),
            events: self.events.clone(),
            artifact_path,
            artifacts: self.artifacts.clone(),
            failure_class: self.failure_class,
            recoverable: status.is_recoverable(),
            recovery_disposition: if status.is_recoverable() {
                Some(RecoveryDisposition::Recoverable)
            } else {
                Some(RecoveryDisposition::NotRecoverable)
            },
        }
    }
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::RunJournalBuilder;
    use everything_domain::{ExecutionMode, RunStatus};
    use std::collections::HashSet;

    #[test]
    fn run_ids_are_unique_under_concurrency() {
        let handles = (0..64)
            .map(|_| std::thread::spawn(|| RunJournalBuilder::new("objective", "model").run_id))
            .collect::<Vec<_>>();
        let ids = handles
            .into_iter()
            .map(|handle| handle.join().expect("run id thread"))
            .collect::<HashSet<_>>();

        assert_eq!(ids.len(), 64);
    }

    #[test]
    fn event_sequences_and_metadata_are_stable() {
        let mut builder =
            RunJournalBuilder::new_for_task("objective", "model", ExecutionMode::Balanced);
        let first = builder.push("bootstrap", "started");
        let second = builder.push("plan", "completed");
        let journal = builder.build(RunStatus::Completed, None);

        assert_eq!(first.sequence, 0);
        assert_eq!(second.sequence, 1);
        assert_eq!(journal.mode, Some(ExecutionMode::Balanced));
        assert!(!journal.recoverable);
        assert!(journal.updated_at_epoch_millis >= journal.created_at_epoch_millis);
    }
}
