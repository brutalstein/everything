use anyhow::Result;
use everything_domain::{EventSeverity, RecoveryDisposition, RunEvent, RunJournal, RunStatus};
use everything_state::StateStore;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct RecoveryReport {
    pub interrupted_runs_reconciled: usize,
}

pub fn reconcile_interrupted_runs(store: &StateStore) -> Result<RecoveryReport> {
    let mut report = RecoveryReport::default();
    for mut journal in store.recoverable_journals()? {
        if !was_interrupted(journal.status) {
            continue;
        }

        let previous_status = journal.status;
        let safe_checkpoint = store.latest_safe_checkpoint(&journal.run_id)?;
        journal.status = RunStatus::Blocked;
        journal.recoverable = true;
        journal.recovery_disposition = Some(if safe_checkpoint.is_some() {
            RecoveryDisposition::Recoverable
        } else {
            RecoveryDisposition::ManualReview
        });
        journal.updated_at_epoch_millis = now_epoch_millis();
        append_recovery_event(&mut journal, previous_status, safe_checkpoint.as_ref());
        store.save_journal(&journal)?;
        report.interrupted_runs_reconciled += 1;
    }
    Ok(report)
}

fn was_interrupted(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Queued
            | RunStatus::Started
            | RunStatus::Running
            | RunStatus::Cancelling
            | RunStatus::Recovering
    )
}

fn append_recovery_event(
    journal: &mut RunJournal,
    previous_status: RunStatus,
    safe_checkpoint: Option<&everything_domain::Checkpoint>,
) {
    let sequence = journal
        .events
        .iter()
        .map(|event| event.sequence)
        .max()
        .map_or(0, |sequence| sequence.saturating_add(1));
    journal.events.push(RunEvent {
        sequence,
        event_id: format!("{}-event-{sequence}", journal.run_id),
        timestamp_epoch_millis: now_epoch_millis(),
        stage: "recovery".to_owned(),
        stage_id: Some(format!("{}:recovery", journal.run_id)),
        correlation_id: Some(journal.run_id.clone()),
        event_kind: "run.interrupted".to_owned(),
        severity: EventSeverity::Warning,
        summary: if safe_checkpoint.is_some() {
            "Interrupted run has a safe checkpoint".to_owned()
        } else {
            "Interrupted run requires review".to_owned()
        },
        detail: format!(
            "previous_status={previous_status:?} disposition={} checkpoint={}",
            if safe_checkpoint.is_some() {
                "recoverable"
            } else {
                "manual_review"
            },
            safe_checkpoint
                .map(|checkpoint| checkpoint.checkpoint_id.as_str())
                .unwrap_or("none")
        ),
        payload: json!({
            "previous_status": format!("{previous_status:?}"),
            "recovery_disposition": if safe_checkpoint.is_some() {
                "Recoverable"
            } else {
                "ManualReview"
            },
            "checkpoint_id": safe_checkpoint
                .map(|checkpoint| checkpoint.checkpoint_id.as_str())
        }),
        provenance: "everything-runtime/recovery".to_owned(),
    });
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::reconcile_interrupted_runs;
    use everything_domain::{
        Checkpoint, CheckpointId, RecoveryDisposition, RunId, RunJournal, RunStatus,
    };
    use everything_state::StateStore;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn interrupted_run_is_blocked_for_manual_review() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-recovery-{suffix}"));
        let store = StateStore::new(root.join("runtime.sqlite3")).expect("state store");
        store
            .save_journal(&RunJournal {
                schema_version: 2,
                run_id: "run-interrupted".to_owned(),
                objective: "test recovery".to_owned(),
                status: RunStatus::Running,
                generated_by: "model".to_owned(),
                mode: None,
                created_at_epoch_millis: 1,
                updated_at_epoch_millis: 2,
                events: Vec::new(),
                artifact_path: None,
                artifacts: Vec::new(),
                failure_class: None,
                recoverable: true,
                recovery_disposition: Some(RecoveryDisposition::Recoverable),
            })
            .expect("save interrupted run");

        let report = reconcile_interrupted_runs(&store).expect("reconcile");
        assert_eq!(report.interrupted_runs_reconciled, 1);
        let journal = store
            .get_journal("run-interrupted")
            .expect("load")
            .expect("run");
        assert_eq!(journal.status, RunStatus::Blocked);
        assert_eq!(
            journal.recovery_disposition,
            Some(RecoveryDisposition::ManualReview)
        );
        assert_eq!(
            journal.events.last().expect("event").event_kind,
            "run.interrupted"
        );

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn interrupted_run_with_safe_checkpoint_is_recoverable() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-recovery-safe-{suffix}"));
        let store = StateStore::new(root.join("runtime.sqlite3")).expect("state store");
        let mut journal = RunJournal {
            schema_version: 2,
            run_id: "run-safe".to_owned(),
            objective: "test recovery".to_owned(),
            status: RunStatus::Running,
            generated_by: "model".to_owned(),
            mode: None,
            created_at_epoch_millis: 1,
            updated_at_epoch_millis: 2,
            events: Vec::new(),
            artifact_path: None,
            artifacts: Vec::new(),
            failure_class: None,
            recoverable: true,
            recovery_disposition: Some(RecoveryDisposition::Recoverable),
        };
        store.save_journal(&journal).expect("save interrupted run");
        store
            .save_checkpoint(&Checkpoint {
                checkpoint_id: CheckpointId::from("checkpoint-safe"),
                run_id: RunId::from("run-safe"),
                stage_id: None,
                event_sequence: 0,
                created_at_epoch_millis: 2,
                safe_to_resume: true,
                summary: "safe boundary".to_owned(),
                artifact_ids: Vec::new(),
            })
            .expect("save checkpoint");

        let report = reconcile_interrupted_runs(&store).expect("reconcile");
        assert_eq!(report.interrupted_runs_reconciled, 1);
        journal = store.get_journal("run-safe").expect("load").expect("run");
        assert_eq!(
            journal.recovery_disposition,
            Some(RecoveryDisposition::Recoverable)
        );
        assert!(
            journal
                .events
                .last()
                .expect("event")
                .detail
                .contains("checkpoint-safe")
        );

        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
