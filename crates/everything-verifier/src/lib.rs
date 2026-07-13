use everything_domain::{
    ArtifactId, InvocationStatus, RunId, ToolInvocationRecord, VerificationCheck,
    VerificationCheckKind, VerificationClaim, VerificationReport, VerificationStatus,
};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct VerificationStepEvidence {
    pub label: String,
    pub invocation: ToolInvocationRecord,
    pub report_artifact_id: ArtifactId,
}

#[derive(Debug, Clone)]
pub struct PatchVerificationContext {
    pub run_id: RunId,
    pub objective_present: bool,
    pub expected_hash_present: bool,
    pub approval_granted: bool,
    pub patch_invocation: ToolInvocationRecord,
    pub diff_artifact_id: Option<ArtifactId>,
    pub verification_steps: Vec<VerificationStepEvidence>,
    pub rolled_back: bool,
    pub rollback_error: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicVerifier;

impl DeterministicVerifier {
    pub fn verify_patch(&self, context: PatchVerificationContext) -> VerificationReport {
        let now = now_millis();
        let mut checks = Vec::new();
        let mut risks = Vec::new();

        let input_ok = context.objective_present && context.expected_hash_present;
        checks.push(VerificationCheck {
            check_id: "input-schema".to_owned(),
            kind: VerificationCheckKind::InputSchema,
            status: pass_fail(input_ok),
            required: true,
            summary: if input_ok {
                "objective and expected content hash were supplied".to_owned()
            } else {
                "objective or expected content hash is missing".to_owned()
            },
            evidence_ids: Vec::new(),
            artifact_ids: Vec::new(),
            invocation_ids: Vec::new(),
            skip_reason: None,
        });

        let permission_blocked = context.patch_invocation.status == InvocationStatus::Failed
            && !context.approval_granted
            && context
                .patch_invocation
                .failure_class
                .is_some_and(|class| class == everything_domain::FailureClass::Permission);
        checks.push(VerificationCheck {
            check_id: "write-precondition".to_owned(),
            kind: VerificationCheckKind::Precondition,
            status: if permission_blocked {
                VerificationStatus::Blocked
            } else {
                VerificationStatus::Passed
            },
            required: true,
            summary: if permission_blocked {
                "workspace write is blocked pending explicit approval".to_owned()
            } else {
                "workspace write preconditions were satisfied".to_owned()
            },
            evidence_ids: Vec::new(),
            artifact_ids: Vec::new(),
            invocation_ids: vec![context.patch_invocation.invocation_id.clone()],
            skip_reason: None,
        });

        let patch_completed = context.patch_invocation.status == InvocationStatus::Completed;
        checks.push(VerificationCheck {
            check_id: "patch-tool-result".to_owned(),
            kind: VerificationCheckKind::ToolResult,
            status: invocation_status(context.patch_invocation.status),
            required: true,
            summary: context
                .patch_invocation
                .result_summary
                .clone()
                .unwrap_or_else(|| "patch tool invocation result".to_owned()),
            evidence_ids: Vec::new(),
            artifact_ids: Vec::new(),
            invocation_ids: vec![context.patch_invocation.invocation_id.clone()],
            skip_reason: None,
        });

        checks.push(VerificationCheck {
            check_id: "diff-artifact".to_owned(),
            kind: VerificationCheckKind::FileDiff,
            status: if !patch_completed {
                VerificationStatus::Skipped
            } else if context.diff_artifact_id.is_some() {
                VerificationStatus::Passed
            } else {
                VerificationStatus::Failed
            },
            required: patch_completed,
            summary: if context.diff_artifact_id.is_some() {
                "content-addressed diff artifact was persisted".to_owned()
            } else {
                "no diff artifact is available".to_owned()
            },
            evidence_ids: Vec::new(),
            artifact_ids: context.diff_artifact_id.clone().into_iter().collect(),
            invocation_ids: vec![context.patch_invocation.invocation_id.clone()],
            skip_reason: (!patch_completed).then(|| "patch did not complete".to_owned()),
        });

        for (index, step) in context.verification_steps.iter().enumerate() {
            let kind = classify_command(&step.label, &step.invocation.tool_name);
            checks.push(VerificationCheck {
                check_id: format!("command-{index}"),
                kind,
                status: invocation_status(step.invocation.status),
                required: true,
                summary: format!(
                    "{}: {}",
                    step.label,
                    step.invocation
                        .result_summary
                        .clone()
                        .unwrap_or_else(|| format!("{:?}", step.invocation.status))
                ),
                evidence_ids: Vec::new(),
                artifact_ids: vec![step.report_artifact_id.clone()],
                invocation_ids: vec![step.invocation.invocation_id.clone()],
                skip_reason: None,
            });
        }

        if context.verification_steps.is_empty() && patch_completed {
            risks.push("no build, test, or lint command was requested".to_owned());
        }
        if context.rolled_back {
            checks.push(VerificationCheck {
                check_id: "automatic-rollback".to_owned(),
                kind: VerificationCheckKind::Recovery,
                status: if context.rollback_error.is_none() {
                    VerificationStatus::Passed
                } else {
                    VerificationStatus::Failed
                },
                required: true,
                summary: context.rollback_error.clone().map_or_else(
                    || "workspace was restored after verification failure".to_owned(),
                    |error| format!("automatic rollback failed: {error}"),
                ),
                evidence_ids: Vec::new(),
                artifact_ids: Vec::new(),
                invocation_ids: Vec::new(),
                skip_reason: None,
            });
        }

        let required_failed = checks.iter().any(|check| {
            check.required
                && matches!(
                    check.status,
                    VerificationStatus::Failed | VerificationStatus::Error
                )
        });
        let required_blocked = checks
            .iter()
            .any(|check| check.required && matches!(check.status, VerificationStatus::Blocked));
        let required_inconclusive = checks.iter().any(|check| {
            check.required && matches!(check.status, VerificationStatus::Inconclusive)
        });
        let status = if required_blocked {
            VerificationStatus::Blocked
        } else if required_failed {
            VerificationStatus::Failed
        } else if required_inconclusive {
            VerificationStatus::Inconclusive
        } else if context.rolled_back {
            VerificationStatus::Failed
        } else {
            VerificationStatus::Passed
        };

        let artifact_ids = checks
            .iter()
            .flat_map(|check| check.artifact_ids.iter().cloned())
            .collect::<Vec<_>>();
        let confidence = match status {
            VerificationStatus::Passed if context.verification_steps.is_empty() => 0.65,
            VerificationStatus::Passed => 0.95,
            VerificationStatus::Blocked => 1.0,
            VerificationStatus::Failed
                if context.rolled_back && context.rollback_error.is_none() =>
            {
                0.95
            }
            VerificationStatus::Failed | VerificationStatus::Error => 0.9,
            VerificationStatus::Inconclusive | VerificationStatus::Skipped => 0.5,
        };
        let claim = match status {
            VerificationStatus::Passed => VerificationClaim {
                claim_id: "patch-verified".to_owned(),
                statement: if context.verification_steps.is_empty() {
                    "the requested patch was applied and its diff was recorded; no external verification command was run".to_owned()
                } else {
                    "the requested patch was applied and all required verification commands completed".to_owned()
                },
                status,
                evidence_ids: Vec::new(),
                artifact_ids,
                confidence,
            },
            VerificationStatus::Blocked => VerificationClaim {
                claim_id: "patch-blocked".to_owned(),
                statement: "the patch was not applied because a required precondition was blocked"
                    .to_owned(),
                status,
                evidence_ids: Vec::new(),
                artifact_ids,
                confidence,
            },
            _ if context.rolled_back && context.rollback_error.is_none() => VerificationClaim {
                claim_id: "patch-rolled-back".to_owned(),
                statement:
                    "verification failed and the workspace was restored to the pre-patch content"
                        .to_owned(),
                status: VerificationStatus::Passed,
                evidence_ids: Vec::new(),
                artifact_ids,
                confidence: 0.95,
            },
            _ => VerificationClaim {
                claim_id: "patch-not-verified".to_owned(),
                statement: "the requested patch could not be verified successfully".to_owned(),
                status,
                evidence_ids: Vec::new(),
                artifact_ids,
                confidence,
            },
        };

        VerificationReport {
            report_id: format!("verification-{}-{now}", context.run_id),
            run_id: context.run_id,
            status,
            generated_at_epoch_millis: now,
            checks,
            claims: vec![claim],
            confidence,
            unresolved_risks: risks,
        }
    }
}

fn pass_fail(value: bool) -> VerificationStatus {
    if value {
        VerificationStatus::Passed
    } else {
        VerificationStatus::Failed
    }
}

fn invocation_status(status: InvocationStatus) -> VerificationStatus {
    match status {
        InvocationStatus::Completed => VerificationStatus::Passed,
        InvocationStatus::Failed => VerificationStatus::Failed,
        InvocationStatus::Cancelled => VerificationStatus::Blocked,
        InvocationStatus::Queued | InvocationStatus::Running => VerificationStatus::Inconclusive,
    }
}

fn classify_command(label: &str, tool_name: &str) -> VerificationCheckKind {
    let value = format!("{label} {tool_name}").to_ascii_lowercase();
    if value.contains("lint") || value.contains("ruff") || value.contains("clippy") {
        VerificationCheckKind::Lint
    } else if value.contains("test") || value.contains("pytest") {
        VerificationCheckKind::Test
    } else if value.contains("build") || value.contains("check") || value.contains("typecheck") {
        VerificationCheckKind::Build
    } else {
        VerificationCheckKind::SuccessCriterion
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::{DeterministicVerifier, PatchVerificationContext};
    use everything_domain::{
        FailureClass, InvocationId, InvocationStatus, PermissionScope, RunId, ToolEffect,
        ToolInvocationRecord, VerificationStatus,
    };
    use serde_json::Value;

    fn invocation(status: InvocationStatus) -> ToolInvocationRecord {
        ToolInvocationRecord {
            invocation_id: InvocationId::new("inv-1"),
            run_id: RunId::new("run-1"),
            stage_id: None,
            tool_name: "workspace.apply_patch".to_owned(),
            tool_version: "1".to_owned(),
            required_permissions: vec![PermissionScope::WorkspaceWrite],
            effect: ToolEffect::WorkspaceMutation,
            status,
            started_at_epoch_millis: 1,
            finished_at_epoch_millis: Some(2),
            arguments: Value::Null,
            output: Value::Null,
            output_truncated: false,
            timeout_millis: None,
            replay_key: None,
            result_summary: None,
            failure_class: None,
            error_code: None,
        }
    }

    #[test]
    fn completed_patch_with_diff_passes_with_explicit_risk_when_commands_absent() {
        let report = DeterministicVerifier.verify_patch(PatchVerificationContext {
            run_id: RunId::new("run-1"),
            objective_present: true,
            expected_hash_present: true,
            approval_granted: true,
            patch_invocation: invocation(InvocationStatus::Completed),
            diff_artifact_id: Some(everything_domain::ArtifactId::new("artifact-1")),
            verification_steps: Vec::new(),
            rolled_back: false,
            rollback_error: None,
        });
        assert_eq!(report.status, VerificationStatus::Passed);
        assert_eq!(report.unresolved_risks.len(), 1);
    }

    #[test]
    fn missing_approval_is_blocked() {
        let mut patch = invocation(InvocationStatus::Failed);
        patch.failure_class = Some(FailureClass::Permission);
        let report = DeterministicVerifier.verify_patch(PatchVerificationContext {
            run_id: RunId::new("run-1"),
            objective_present: true,
            expected_hash_present: true,
            approval_granted: false,
            patch_invocation: patch,
            diff_artifact_id: None,
            verification_steps: Vec::new(),
            rolled_back: false,
            rollback_error: None,
        });
        assert_eq!(report.status, VerificationStatus::Blocked);
    }
}
