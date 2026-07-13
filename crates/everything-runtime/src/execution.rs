use crate::{ModularRuntime, journal::RunJournalBuilder};
use anyhow::{Context, Result, anyhow};
use everything_domain::{
    ArtifactDescriptor, ArtifactId, ArtifactKind, Checkpoint, CheckpointId, ErrorCode,
    EventSeverity, FailureClass, InvocationId, InvocationStatus, PatchExecutionRequest,
    PatchExecutionResponse, RunEvent, RunId, RunJournal, RunStatus, StageId, ToolDefinition,
    ToolInvocationRecord, ToolInvocationRequest, ToolInvocationResponse,
};
use everything_graph::{ChangeKind, CodeGraphChangeImpactRequest, CodeGraphChangeTarget};
use everything_verifier::{PatchVerificationContext, VerificationStepEvidence};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static INVOCATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl ModularRuntime {
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_runtime.definitions().to_vec()
    }

    pub fn invoke_tool(
        &self,
        invocation_id: InvocationId,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse> {
        let Some(mut journal) = self.state_store.get_journal(request.run_id.as_str())? else {
            return Err(anyhow!("run '{}' not found", request.run_id));
        };
        let running = self
            .tool_runtime
            .running_record(invocation_id.clone(), &request)?;
        self.state_store.save_tool_invocation(&running)?;
        let response = match self.tool_runtime.invoke(invocation_id, request) {
            Ok(response) => response,
            Err(error) => ToolInvocationResponse {
                invocation: failed_from_running(running, error.to_string()),
                output: json!({"error": error.to_string()}),
            },
        };
        self.state_store
            .save_tool_invocation(&response.invocation)?;
        append_invocation_event(&mut journal, &response.invocation);
        self.state_store.save_journal(&journal)?;
        Ok(response)
    }

    pub fn cancel_tool_invocation(&self, invocation_id: &str) -> bool {
        self.tool_runtime.cancel(invocation_id)
    }

    pub fn get_tool_invocation(&self, invocation_id: &str) -> Result<Option<ToolInvocationRecord>> {
        self.state_store.get_tool_invocation(invocation_id)
    }

    pub fn list_tool_invocations(&self, run_id: &str) -> Result<Vec<ToolInvocationRecord>> {
        self.state_store.list_tool_invocations(run_id)
    }

    pub fn execute_patch(&self, request: &PatchExecutionRequest) -> Result<PatchExecutionResponse> {
        anyhow::ensure!(
            !request.objective.trim().is_empty(),
            "objective must not be empty"
        );
        let tool_budget = match request.mode {
            everything_domain::ExecutionMode::Fast => 4usize,
            everything_domain::ExecutionMode::Balanced => 12usize,
            everything_domain::ExecutionMode::Deep => 32usize,
        };
        anyhow::ensure!(
            request.verification_commands.len().saturating_add(1) <= tool_budget,
            "{} mode permits at most {} tool invocations for a patch transaction",
            request.mode,
            tool_budget
        );
        let mut journal = RunJournalBuilder::new_for_task(
            &request.objective,
            "deterministic-tool-engine",
            request.mode,
        );
        journal.push_structured(
            "execution",
            "execution.started",
            EventSeverity::Info,
            "Patch execution started",
            format!("path={}", request.relative_path.display()),
            json!({
                "path": request.relative_path,
                "mode": request.mode,
                "tool_budget": tool_budget,
            }),
            "everything-runtime",
        );
        self.state_store
            .save_journal(&journal.snapshot(RunStatus::Running, None))?;

        let mut preflight_artifacts = Vec::new();
        match self
            .persistent_graph
            .index_workspace(&self.settings.workspace_path)
            .and_then(|_| {
                self.persistent_graph
                    .analyze_change(&CodeGraphChangeImpactRequest {
                        targets: vec![CodeGraphChangeTarget {
                            file_path: Some(request.relative_path.clone()),
                            symbol: None,
                            start_line: None,
                            end_line: None,
                            change_kind: ChangeKind::Modify,
                        }],
                        max_depth: match request.mode {
                            everything_domain::ExecutionMode::Fast => 2,
                            everything_domain::ExecutionMode::Balanced => 4,
                            everything_domain::ExecutionMode::Deep => 6,
                        },
                        max_entities: match request.mode {
                            everything_domain::ExecutionMode::Fast => 96,
                            everything_domain::ExecutionMode::Balanced => 256,
                            everything_domain::ExecutionMode::Deep => 768,
                        },
                        include_inferred: request.mode == everything_domain::ExecutionMode::Deep,
                    })
            }) {
            Ok(report) => {
                let artifact = self.persist_execution_artifact(
                    journal.run_id(),
                    ArtifactKind::ImpactReport,
                    "application/json",
                    "patch-preflight-impact",
                    &serde_json::to_vec_pretty(&report)?,
                )?;
                journal.add_artifact(artifact.clone());
                preflight_artifacts.push(artifact);
                journal.push_structured(
                    "impact",
                    "impact.preflight.completed",
                    EventSeverity::Info,
                    "Change impact preflight completed",
                    format!(
                        "risk={:?} affected_files={} verifier_targets={}",
                        report.risk_tier,
                        report.affected_files.len(),
                        report.verification_targets.len()
                    ),
                    serde_json::to_value(&report)?,
                    "everything-graph",
                );
            }
            Err(error) => {
                journal.push_structured(
                    "impact",
                    "impact.preflight.degraded",
                    EventSeverity::Warning,
                    "Change impact preflight unavailable",
                    error.to_string(),
                    json!({
                        "path": request.relative_path,
                        "error": error.to_string(),
                    }),
                    "everything-graph",
                );
            }
        }
        self.state_store
            .save_journal(&journal.snapshot(RunStatus::Running, None))?;

        let run_id = RunId::new(journal.run_id());
        let patch_invocation_id = next_invocation_id(journal.run_id(), "patch");
        let patch_request = ToolInvocationRequest {
            run_id: run_id.clone(),
            tool_id: "workspace.apply_patch".to_owned(),
            input: json!({
                "path": request.relative_path,
                "expected_content_hash": request.expected_content_hash,
                "replacement_content": request.replacement_content,
            }),
            approval_granted: request.approval_granted,
            timeout_millis: None,
        };
        let patch_running = self
            .tool_runtime
            .running_record(patch_invocation_id.clone(), &patch_request)?;
        if !request.allow_repeat_failure {
            if let Some(replay_key) = patch_running.replay_key.as_deref() {
                if let Some(previous) = self
                    .state_store
                    .latest_failed_run_invocation_by_replay_key(replay_key)?
                {
                    let mut blocked_invocation = patch_running.clone();
                    blocked_invocation.status = InvocationStatus::Failed;
                    blocked_invocation.finished_at_epoch_millis = Some(now_millis());
                    blocked_invocation.output = json!({
                        "error": "identical failed patch transaction requires explicit override",
                        "previous_run_id": previous.run_id.as_str(),
                        "previous_invocation_id": previous.invocation_id.as_str(),
                        "replay_key": replay_key,
                    });
                    blocked_invocation.result_summary = Some(
                        "identical patch was already part of a failed run; blind replay blocked"
                            .to_owned(),
                    );
                    blocked_invocation.failure_class = Some(FailureClass::Validation);
                    blocked_invocation.error_code =
                        Some(ErrorCode::new("repeat_failed_patch_blocked"));
                    self.state_store.save_tool_invocation(&blocked_invocation)?;
                    push_invocation_event(&mut journal, &blocked_invocation);
                    journal.set_failure_class(FailureClass::Validation);
                    journal.push_structured(
                        "execution",
                        "execution.replay_blocked",
                        EventSeverity::Warning,
                        "Blind patch replay blocked",
                        format!(
                            "previous_run={} previous_invocation={}",
                            previous.run_id, previous.invocation_id
                        ),
                        blocked_invocation.output.clone(),
                        "everything-runtime",
                    );
                    let verification_report =
                        self.verifier.verify_patch(PatchVerificationContext {
                            run_id: run_id.clone(),
                            objective_present: !request.objective.trim().is_empty(),
                            expected_hash_present: !request.expected_content_hash.trim().is_empty(),
                            approval_granted: request.approval_granted,
                            patch_invocation: blocked_invocation,
                            diff_artifact_id: None,
                            verification_steps: Vec::new(),
                            rolled_back: false,
                            rollback_error: None,
                        });
                    let verification_artifact = self.persist_execution_artifact(
                        journal.run_id(),
                        ArtifactKind::VerificationReport,
                        "application/json",
                        "deterministic-verifier",
                        &serde_json::to_vec_pretty(&verification_report)?,
                    )?;
                    journal.add_artifact(verification_artifact.clone());
                    self.state_store
                        .save_journal(&journal.snapshot(RunStatus::Blocked, None))?;
                    return Ok(PatchExecutionResponse {
                        run_id,
                        status: status_name(RunStatus::Blocked),
                        patch_invocation_id,
                        verification_invocation_ids: Vec::new(),
                        artifacts: preflight_artifacts
                            .iter()
                            .cloned()
                            .chain(std::iter::once(verification_artifact.clone()))
                            .collect(),
                        rolled_back: false,
                        summary: "identical failed patch transaction was blocked; set allow_repeat_failure only after reviewing the previous evidence".to_owned(),
                        verification_report: Some(verification_report),
                        verification_artifact_id: Some(ArtifactId::new(
                            verification_artifact.artifact_id,
                        )),
                    });
                }
            }
        }
        self.state_store.save_tool_invocation(&patch_running)?;
        let (patch_response, receipt) = self
            .tool_runtime
            .invoke_patch_with_receipt(patch_invocation_id.clone(), patch_request)?;
        self.state_store
            .save_tool_invocation(&patch_response.invocation)?;
        push_invocation_event(&mut journal, &patch_response.invocation);

        if patch_response.invocation.status != InvocationStatus::Completed {
            let failure = patch_response
                .invocation
                .failure_class
                .unwrap_or(FailureClass::Tool);
            journal.set_failure_class(failure);
            let status = if failure == FailureClass::Permission {
                RunStatus::AwaitingApproval
            } else {
                RunStatus::Failed
            };
            journal.push_structured(
                "execution",
                "execution.blocked",
                EventSeverity::Error,
                "Patch execution did not start",
                patch_response
                    .invocation
                    .result_summary
                    .clone()
                    .unwrap_or_else(|| "patch invocation failed".to_owned()),
                patch_response.output.clone(),
                "everything-tools",
            );
            let verification_report = self.verifier.verify_patch(PatchVerificationContext {
                run_id: run_id.clone(),
                objective_present: !request.objective.trim().is_empty(),
                expected_hash_present: !request.expected_content_hash.trim().is_empty(),
                approval_granted: request.approval_granted,
                patch_invocation: patch_response.invocation.clone(),
                diff_artifact_id: None,
                verification_steps: Vec::new(),
                rolled_back: false,
                rollback_error: None,
            });
            let verification_artifact = self.persist_execution_artifact(
                journal.run_id(),
                ArtifactKind::VerificationReport,
                "application/json",
                "deterministic-verifier",
                &serde_json::to_vec_pretty(&verification_report)?,
            )?;
            journal.add_artifact(verification_artifact.clone());
            journal.push_structured(
                "verification",
                "verification.reported",
                EventSeverity::Info,
                "Verification report persisted",
                format!("status={:?}", verification_report.status),
                serde_json::to_value(&verification_report)?,
                "everything-verifier",
            );
            self.state_store
                .save_journal(&journal.snapshot(status, None))?;
            return Ok(PatchExecutionResponse {
                run_id,
                status: status_name(status),
                patch_invocation_id,
                verification_invocation_ids: Vec::new(),
                artifacts: preflight_artifacts
                    .iter()
                    .cloned()
                    .chain(std::iter::once(verification_artifact.clone()))
                    .collect(),
                rolled_back: false,
                summary: if status == RunStatus::AwaitingApproval {
                    "workspace write requires explicit operator approval".to_owned()
                } else {
                    "patch invocation failed".to_owned()
                },
                verification_report: Some(verification_report),
                verification_artifact_id: Some(ArtifactId::new(verification_artifact.artifact_id)),
            });
        }

        let receipt = receipt.context("completed patch invocation did not return a receipt")?;
        let mut artifacts = preflight_artifacts;
        let diff = patch_response
            .output
            .get("diff")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let diff_artifact = self.persist_execution_artifact(
            journal.run_id(),
            ArtifactKind::Diff,
            "text/x-diff",
            "workspace.apply_patch",
            diff.as_bytes(),
        )?;
        journal.add_artifact(diff_artifact.clone());
        artifacts.push(diff_artifact.clone());
        journal.push_structured(
            "execution",
            "patch.applied",
            EventSeverity::Info,
            "Patch applied",
            format!("path={}", request.relative_path.display()),
            patch_response.output.clone(),
            "everything-tools",
        );
        let unsafe_checkpoint = Checkpoint {
            checkpoint_id: CheckpointId::new(format!("checkpoint-{}-patch", journal.run_id())),
            run_id: run_id.clone(),
            stage_id: Some(StageId::new(format!("{}:execution", journal.run_id()))),
            event_sequence: journal.last_event_sequence().unwrap_or_default(),
            created_at_epoch_millis: now_millis(),
            safe_to_resume: false,
            summary: "patch applied; verification or rollback still required".to_owned(),
            artifact_ids: vec![ArtifactId::new(diff_artifact.artifact_id.clone())],
        };
        self.state_store.save_checkpoint(&unsafe_checkpoint)?;
        self.state_store
            .save_journal(&journal.snapshot(RunStatus::Running, None))?;

        let mut verification_invocation_ids = Vec::new();
        let mut verification_evidence = Vec::new();
        let mut verification_failed = None;
        for (index, verification) in request.verification_commands.iter().enumerate() {
            let invocation_id = next_invocation_id(journal.run_id(), &format!("verify-{index}"));
            let tool_request = ToolInvocationRequest {
                run_id: run_id.clone(),
                tool_id: "process.run".to_owned(),
                input: json!({
                    "program": verification.program,
                    "args": verification.args,
                    "working_directory": ".",
                }),
                approval_granted: request.approval_granted,
                timeout_millis: verification.timeout_millis,
            };
            let running = self
                .tool_runtime
                .running_record(invocation_id.clone(), &tool_request)?;
            self.state_store.save_tool_invocation(&running)?;
            let response = self
                .tool_runtime
                .invoke_command(invocation_id.clone(), tool_request)?;
            self.state_store
                .save_tool_invocation(&response.invocation)?;
            push_invocation_event(&mut journal, &response.invocation);
            verification_invocation_ids.push(invocation_id);

            let report_content = serde_json::to_vec_pretty(&json!({
                "label": verification.label,
                "command": {"program": verification.program, "args": verification.args},
                "invocation": &response.invocation,
                "output": &response.output,
            }))?;
            let report_artifact = self.persist_execution_artifact(
                journal.run_id(),
                ArtifactKind::TestReport,
                "application/json",
                format!("verification:{}", verification.label),
                &report_content,
            )?;
            journal.add_artifact(report_artifact.clone());
            artifacts.push(report_artifact.clone());
            verification_evidence.push(VerificationStepEvidence {
                label: verification.label.clone(),
                invocation: response.invocation.clone(),
                report_artifact_id: ArtifactId::new(report_artifact.artifact_id),
            });

            if response.invocation.status != InvocationStatus::Completed {
                verification_failed = Some((verification.label.clone(), response.invocation));
                break;
            }
            self.state_store
                .save_journal(&journal.snapshot(RunStatus::Running, None))?;
        }

        let mut rolled_back = false;
        let mut rollback_error = None;
        let (final_status, summary) = if let Some((label, failed_invocation)) = verification_failed
        {
            let rollback_result = self.tool_runtime.rollback_patch(&receipt);
            rolled_back = rollback_result.is_ok();
            rollback_error = rollback_result.as_ref().err().map(ToString::to_string);
            let rollback_payload = json!({
                "path": request.relative_path,
                "verification": label,
                "rolled_back": rolled_back,
                "rollback_error": rollback_result.as_ref().err().map(ToString::to_string),
            });
            let rollback_artifact = self.persist_execution_artifact(
                journal.run_id(),
                ArtifactKind::Log,
                "application/json",
                "automatic-rollback",
                &serde_json::to_vec_pretty(&rollback_payload)?,
            )?;
            journal.add_artifact(rollback_artifact.clone());
            artifacts.push(rollback_artifact);
            journal.set_failure_class(
                failed_invocation
                    .failure_class
                    .unwrap_or(FailureClass::Validation),
            );
            journal.push_structured(
                "verification",
                "verification.failed",
                EventSeverity::Error,
                "Verification failed",
                format!("verification={label} rolled_back={rolled_back}"),
                rollback_payload,
                "everything-runtime",
            );
            if let Err(error) = rollback_result {
                journal.set_failure_class(FailureClass::Internal);
                (
                    RunStatus::Failed,
                    format!("verification failed and rollback failed: {error}"),
                )
            } else {
                (
                    RunStatus::Failed,
                    format!("verification '{label}' failed; patch rolled back"),
                )
            }
        } else {
            journal.push_structured(
                "verification",
                "verification.completed",
                EventSeverity::Info,
                "Verification completed",
                format!("commands={}", request.verification_commands.len()),
                json!({"commands": request.verification_commands.len()}),
                "everything-runtime",
            );
            (
                RunStatus::Completed,
                "patch applied and verification completed".to_owned(),
            )
        };

        let verification_report = self.verifier.verify_patch(PatchVerificationContext {
            run_id: run_id.clone(),
            objective_present: !request.objective.trim().is_empty(),
            expected_hash_present: !request.expected_content_hash.trim().is_empty(),
            approval_granted: request.approval_granted,
            patch_invocation: patch_response.invocation.clone(),
            diff_artifact_id: Some(ArtifactId::new(diff_artifact.artifact_id.clone())),
            verification_steps: verification_evidence,
            rolled_back,
            rollback_error,
        });
        let verification_artifact = self.persist_execution_artifact(
            journal.run_id(),
            ArtifactKind::VerificationReport,
            "application/json",
            "deterministic-verifier",
            &serde_json::to_vec_pretty(&verification_report)?,
        )?;
        journal.add_artifact(verification_artifact.clone());
        artifacts.push(verification_artifact.clone());
        journal.push_structured(
            "verification",
            "verification.reported",
            if verification_report.required_checks_passed() {
                EventSeverity::Info
            } else {
                EventSeverity::Error
            },
            "Verification report persisted",
            format!(
                "status={:?} confidence={:.2} checks={}",
                verification_report.status,
                verification_report.confidence,
                verification_report.checks.len()
            ),
            serde_json::to_value(&verification_report)?,
            "everything-verifier",
        );

        match self
            .persistent_graph
            .index_workspace(&self.settings.workspace_path)
        {
            Ok(_) if !rolled_back => {
                let (start_line, end_line) = changed_line_range(diff);
                match self
                    .persistent_graph
                    .analyze_change(&CodeGraphChangeImpactRequest {
                        targets: vec![CodeGraphChangeTarget {
                            file_path: Some(request.relative_path.clone()),
                            symbol: None,
                            start_line,
                            end_line,
                            change_kind: ChangeKind::Modify,
                        }],
                        max_depth: match request.mode {
                            everything_domain::ExecutionMode::Fast => 2,
                            everything_domain::ExecutionMode::Balanced => 4,
                            everything_domain::ExecutionMode::Deep => 6,
                        },
                        max_entities: match request.mode {
                            everything_domain::ExecutionMode::Fast => 96,
                            everything_domain::ExecutionMode::Balanced => 256,
                            everything_domain::ExecutionMode::Deep => 768,
                        },
                        include_inferred: request.mode == everything_domain::ExecutionMode::Deep,
                    }) {
                    Ok(report) => {
                        let artifact = self.persist_execution_artifact(
                            journal.run_id(),
                            ArtifactKind::ImpactReport,
                            "application/json",
                            "patch-postflight-impact",
                            &serde_json::to_vec_pretty(&report)?,
                        )?;
                        journal.add_artifact(artifact.clone());
                        artifacts.push(artifact);
                        journal.push_structured(
                            "impact",
                            "impact.postflight.completed",
                            EventSeverity::Info,
                            "Post-change impact map refreshed",
                            format!(
                                "risk={:?} affected_files={} affected_entities={}",
                                report.risk_tier,
                                report.affected_files.len(),
                                report.affected_entities.len()
                            ),
                            serde_json::to_value(&report)?,
                            "everything-graph",
                        );
                    }
                    Err(error) => {
                        journal.push_structured(
                            "impact",
                            "impact.postflight.degraded",
                            EventSeverity::Warning,
                            "Post-change impact analysis unavailable",
                            error.to_string(),
                            json!({"path": request.relative_path, "error": error.to_string()}),
                            "everything-graph",
                        );
                    }
                }
            }
            Ok(_) => {
                journal.push_structured(
                    "graph",
                    "graph.refresh.completed",
                    EventSeverity::Info,
                    "Graph refreshed after rollback",
                    format!("path={}", request.relative_path.display()),
                    Value::Null,
                    "everything-graph",
                );
            }
            Err(error) => {
                journal.push_structured(
                    "graph",
                    "graph.refresh.failed",
                    EventSeverity::Warning,
                    "Graph refresh failed",
                    error.to_string(),
                    Value::Null,
                    "everything-graph",
                );
            }
        }

        let final_checkpoint = Checkpoint {
            checkpoint_id: CheckpointId::new(format!("checkpoint-{}-final", journal.run_id())),
            run_id: run_id.clone(),
            stage_id: Some(StageId::new(format!("{}:verification", journal.run_id()))),
            event_sequence: journal.last_event_sequence().unwrap_or_default(),
            created_at_epoch_millis: now_millis(),
            safe_to_resume: true,
            summary: if rolled_back {
                "workspace restored after failed verification".to_owned()
            } else {
                "patch transaction reached a stable boundary".to_owned()
            },
            artifact_ids: artifacts
                .iter()
                .map(|artifact| ArtifactId::new(artifact.artifact_id.clone()))
                .collect(),
        };
        self.state_store.save_checkpoint(&final_checkpoint)?;
        let final_journal = journal.build(final_status, Some(diff_artifact.object_path.clone()));
        self.state_store.save_journal(&final_journal)?;

        Ok(PatchExecutionResponse {
            run_id,
            status: status_name(final_status),
            patch_invocation_id,
            verification_invocation_ids,
            artifacts,
            rolled_back,
            summary,
            verification_artifact_id: Some(ArtifactId::new(
                verification_artifact.artifact_id.clone(),
            )),
            verification_report: Some(verification_report),
        })
    }

    fn persist_execution_artifact(
        &self,
        run_id: &str,
        kind: ArtifactKind,
        media_type: &str,
        origin: impl Into<String>,
        content: &[u8],
    ) -> Result<ArtifactDescriptor> {
        let artifact = self
            .artifact_store
            .persist(run_id, kind, media_type, origin, content)?;
        self.state_store.save_artifact(&artifact)?;
        Ok(artifact)
    }
}

fn failed_from_running(
    mut invocation: ToolInvocationRecord,
    message: String,
) -> ToolInvocationRecord {
    invocation.status = InvocationStatus::Failed;
    invocation.finished_at_epoch_millis = Some(now_millis());
    invocation.output = json!({"error": message});
    invocation.result_summary = Some("tool invocation rejected".to_owned());
    invocation.failure_class = Some(FailureClass::Validation);
    invocation.error_code = Some(ErrorCode::new("tool_input_invalid"));
    invocation
}

fn push_invocation_event(journal: &mut RunJournalBuilder, invocation: &ToolInvocationRecord) {
    let severity = match invocation.status {
        InvocationStatus::Completed => EventSeverity::Info,
        InvocationStatus::Cancelled => EventSeverity::Warning,
        InvocationStatus::Queued | InvocationStatus::Running => EventSeverity::Debug,
        InvocationStatus::Failed => EventSeverity::Error,
    };
    journal.push_structured(
        "tool",
        "tool.invocation",
        severity,
        format!("Tool {} {:?}", invocation.tool_name, invocation.status),
        invocation
            .result_summary
            .clone()
            .unwrap_or_else(|| invocation.tool_name.clone()),
        serde_json::to_value(invocation).unwrap_or(Value::Null),
        "everything-tools",
    );
}

fn append_invocation_event(journal: &mut RunJournal, invocation: &ToolInvocationRecord) {
    let sequence = journal
        .events
        .iter()
        .map(|event| event.sequence)
        .max()
        .map_or(0, |value| value.saturating_add(1));
    let severity = match invocation.status {
        InvocationStatus::Completed => EventSeverity::Info,
        InvocationStatus::Cancelled => EventSeverity::Warning,
        InvocationStatus::Queued | InvocationStatus::Running => EventSeverity::Debug,
        InvocationStatus::Failed => EventSeverity::Error,
    };
    journal.events.push(RunEvent {
        sequence,
        event_id: format!("{}-event-{sequence}", journal.run_id),
        timestamp_epoch_millis: now_millis(),
        stage: "tool".to_owned(),
        stage_id: Some(format!("{}:tool", journal.run_id)),
        correlation_id: Some(journal.run_id.clone()),
        event_kind: "tool.invocation".to_owned(),
        severity,
        summary: format!("Tool {} {:?}", invocation.tool_name, invocation.status),
        detail: invocation
            .result_summary
            .clone()
            .unwrap_or_else(|| invocation.tool_name.clone()),
        payload: serde_json::to_value(invocation).unwrap_or(Value::Null),
        provenance: "everything-tools".to_owned(),
    });
    journal.updated_at_epoch_millis = now_millis();
    if !journal.status.is_terminal() {
        journal.status = match invocation.status {
            InvocationStatus::Failed
                if invocation.failure_class == Some(FailureClass::Permission) =>
            {
                RunStatus::AwaitingApproval
            }
            InvocationStatus::Failed => RunStatus::Failed,
            InvocationStatus::Cancelled => RunStatus::Cancelled,
            InvocationStatus::Queued | InvocationStatus::Running | InvocationStatus::Completed => {
                RunStatus::Running
            }
        };
        journal.failure_class = invocation.failure_class;
        journal.recoverable = journal.status.is_recoverable();
    }
}

fn changed_line_range(diff: &str) -> (Option<usize>, Option<usize>) {
    let Some(header) = diff.lines().find(|line| line.starts_with("@@ ")) else {
        return (None, None);
    };
    let Some(new_range) = header.split_whitespace().find(|part| part.starts_with('+')) else {
        return (None, None);
    };
    let range = new_range.trim_start_matches('+');
    let (start, count) = range
        .split_once(',')
        .map_or((range, "1"), |(start, count)| (start, count));
    let Ok(start) = start.parse::<usize>() else {
        return (None, None);
    };
    let count = count.parse::<usize>().unwrap_or(1);
    let end = if count == 0 {
        start
    } else {
        start.saturating_add(count.saturating_sub(1))
    };
    (Some(start), Some(end))
}

fn next_invocation_id(run_id: &str, stage: &str) -> InvocationId {
    let sequence = INVOCATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    InvocationId::new(format!(
        "invocation-{run_id}-{stage}-{}-{sequence}",
        now_millis()
    ))
}

fn status_name(status: RunStatus) -> String {
    match status {
        RunStatus::Queued => "queued",
        RunStatus::Started => "started",
        RunStatus::Running => "running",
        RunStatus::Paused => "paused",
        RunStatus::AwaitingApproval => "awaiting_approval",
        RunStatus::Blocked => "blocked",
        RunStatus::Cancelling => "cancelling",
        RunStatus::Cancelled => "cancelled",
        RunStatus::Recovering => "recovering",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
    }
    .to_owned()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::ModularRuntime;
    use crate::RuntimeComponents;
    use everything_adapters::{LocalCommandExecutor, LocalFileSystemAdapter, LocalModelAdapter};
    use everything_domain::{PatchExecutionRequest, RuntimeSettings, VerificationCommand};
    use everything_graph::PersistentCodeGraph;
    use everything_tools::content_hash;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn runtime(label: &str) -> (std::path::PathBuf, ModularRuntime) {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-execution-{label}-{suffix}"));
        std::fs::create_dir_all(&root).expect("workspace");
        let mut settings = RuntimeSettings::for_workspace(root.clone());
        settings.data_dir = root.join(".everything");
        settings.tools.allowed_programs = vec!["rustc".to_owned()];
        let runtime = ModularRuntime::new(
            RuntimeComponents {
                file_system: Arc::new(LocalFileSystemAdapter::new(settings.data_dir.join("cache"))),
                model: Arc::new(LocalModelAdapter::new("test-model")),
                command: Arc::new(LocalCommandExecutor),
            },
            PersistentCodeGraph::new(settings.data_dir.join("graph/codegraph.sqlite3")),
            settings,
        );
        (root, runtime)
    }

    #[test]
    fn patch_requires_operator_approval_without_mutating_workspace() {
        let (root, runtime) = runtime("approval");
        std::fs::write(root.join("demo.txt"), "before\n").expect("file");
        let response = runtime
            .execute_patch(&PatchExecutionRequest {
                objective: "change demo".to_owned(),
                mode: everything_domain::ExecutionMode::Balanced,
                relative_path: "demo.txt".into(),
                expected_content_hash: content_hash(b"before\n"),
                replacement_content: "after\n".to_owned(),
                verification_commands: Vec::new(),
                approval_granted: false,
                allow_repeat_failure: false,
            })
            .expect("response");

        assert_eq!(response.status, "awaiting_approval");
        assert!(!response.rolled_back);
        assert_eq!(
            std::fs::read_to_string(root.join("demo.txt")).expect("unchanged"),
            "before\n"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn identical_failed_patch_is_not_replayed_without_explicit_override() {
        let (root, runtime) = runtime("replay-guard");
        std::fs::write(root.join("demo.txt"), "before\n").expect("file");
        let mut request = PatchExecutionRequest {
            objective: "change demo".to_owned(),
            mode: everything_domain::ExecutionMode::Balanced,
            relative_path: "demo.txt".into(),
            expected_content_hash: content_hash(b"before\n"),
            replacement_content: "after\n".to_owned(),
            verification_commands: vec![VerificationCommand {
                program: "rustc".to_owned(),
                args: vec!["--definitely-invalid-everything-flag".to_owned()],
                label: "intentional failure".to_owned(),
                timeout_millis: Some(30_000),
            }],
            approval_granted: true,
            allow_repeat_failure: false,
        };

        let first = runtime.execute_patch(&request).expect("first response");
        assert_eq!(first.status, "failed");
        assert!(first.rolled_back);

        let second = runtime.execute_patch(&request).expect("blocked response");
        assert_eq!(second.status, "blocked");
        assert!(!second.rolled_back);
        assert!(second.summary.contains("blocked"));
        assert_eq!(
            std::fs::read_to_string(root.join("demo.txt")).expect("still restored"),
            "before\n"
        );

        request.allow_repeat_failure = true;
        let third = runtime.execute_patch(&request).expect("override response");
        assert_eq!(third.status, "failed");
        assert!(third.rolled_back);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn failed_verification_rolls_back_only_the_applied_patch() {
        let (root, runtime) = runtime("rollback");
        std::fs::write(root.join("demo.txt"), "before\n").expect("file");
        let response = runtime
            .execute_patch(&PatchExecutionRequest {
                objective: "change demo".to_owned(),
                mode: everything_domain::ExecutionMode::Balanced,
                relative_path: "demo.txt".into(),
                expected_content_hash: content_hash(b"before\n"),
                replacement_content: "after\n".to_owned(),
                verification_commands: vec![VerificationCommand {
                    program: "rustc".to_owned(),
                    args: vec!["--definitely-invalid-everything-flag".to_owned()],
                    label: "intentional failure".to_owned(),
                    timeout_millis: Some(30_000),
                }],
                approval_granted: true,
                allow_repeat_failure: false,
            })
            .expect("response");

        assert_eq!(response.status, "failed");
        assert!(response.rolled_back);
        assert_eq!(response.verification_invocation_ids.len(), 1);
        assert_eq!(
            std::fs::read_to_string(root.join("demo.txt")).expect("restored"),
            "before\n"
        );
        let invocations = runtime
            .list_tool_invocations(response.run_id.as_str())
            .expect("invocations");
        assert_eq!(invocations.len(), 2);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
