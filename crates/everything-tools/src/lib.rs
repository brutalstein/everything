mod command;
mod patch;
mod policy;
mod workspace;

pub use command::{CancellationToken, SafeCommandOutput, SafeCommandRequest, SafeCommandRunner};
pub use patch::{PatchPreview, PatchReceipt, PatchTransaction, content_hash};
pub use policy::{PolicyViolation, ToolPolicy};
pub use workspace::{SearchMatch, WorkspaceGuard};

use anyhow::{Result, anyhow};
use everything_domain::{
    ErrorCode, FailureClass, InvocationId, InvocationStatus, PermissionScope, ToolDefinition,
    ToolEffect, ToolInvocationRecord, ToolInvocationRequest, ToolInvocationResponse,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct ToolRuntime {
    guard: WorkspaceGuard,
    patcher: PatchTransaction,
    command_runner: SafeCommandRunner,
    policy: ToolPolicy,
    definitions: Arc<Vec<ToolDefinition>>,
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl ToolRuntime {
    pub fn new(
        workspace: impl AsRef<Path>,
        policy: ToolPolicy,
        allowed_programs: impl IntoIterator<Item = String>,
    ) -> Result<Self> {
        Self::new_with_limits(workspace, policy, allowed_programs, 120_000, 1_000_000)
    }

    pub fn new_with_limits(
        workspace: impl AsRef<Path>,
        policy: ToolPolicy,
        allowed_programs: impl IntoIterator<Item = String>,
        default_timeout_millis: u64,
        max_output_bytes: u64,
    ) -> Result<Self> {
        Self::new_with_security(
            workspace,
            policy,
            allowed_programs,
            default_timeout_millis,
            max_output_bytes,
            false,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_security(
        workspace: impl AsRef<Path>,
        policy: ToolPolicy,
        allowed_programs: impl IntoIterator<Item = String>,
        default_timeout_millis: u64,
        max_output_bytes: u64,
        trusted_workspace: bool,
        os_sandbox_enabled: bool,
    ) -> Result<Self> {
        let guard = WorkspaceGuard::new(workspace)?;
        let patcher = PatchTransaction::new(guard.clone());
        let command_runner = SafeCommandRunner::new_with_security(
            guard.clone(),
            allowed_programs,
            trusted_workspace,
            os_sandbox_enabled,
        );
        Ok(Self {
            guard,
            patcher,
            command_runner,
            policy,
            definitions: Arc::new(builtin_definitions(
                default_timeout_millis.clamp(1, 60 * 60 * 1000),
                max_output_bytes.clamp(1_024, 100_000_000),
            )),
            active: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn definitions(&self) -> &[ToolDefinition] {
        &self.definitions
    }

    pub fn sandbox_description(&self) -> &'static str {
        self.command_runner.sandbox_description()
    }

    pub fn running_record(
        &self,
        invocation_id: InvocationId,
        request: &ToolInvocationRequest,
    ) -> Result<ToolInvocationRecord> {
        let definition = self.definition(&request.tool_id)?;
        Ok(ToolInvocationRecord {
            invocation_id,
            run_id: request.run_id.clone(),
            stage_id: None,
            tool_name: definition.tool_id.clone(),
            tool_version: definition.version.clone(),
            required_permissions: definition.required_permissions.clone(),
            effect: definition.effect,
            status: InvocationStatus::Running,
            started_at_epoch_millis: now_millis(),
            finished_at_epoch_millis: None,
            arguments: request.input.clone(),
            output: Value::Null,
            output_truncated: false,
            timeout_millis: request
                .timeout_millis
                .or(Some(definition.default_timeout_millis)),
            replay_key: Some(replay_key(definition, &request.input)),
            result_summary: Some("tool invocation running".to_owned()),
            failure_class: None,
            error_code: None,
        })
    }

    pub fn invoke(
        &self,
        invocation_id: InvocationId,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse> {
        match request.tool_id.as_str() {
            "workspace.apply_patch" => {
                let (response, _) = self.invoke_patch_with_receipt(invocation_id, request)?;
                Ok(response)
            }
            "process.run" | "git.status" | "git.diff" => {
                self.invoke_command(invocation_id, request)
            }
            _ => self.invoke_read_tool(invocation_id, request),
        }
    }

    pub fn invoke_patch_with_receipt(
        &self,
        invocation_id: InvocationId,
        request: ToolInvocationRequest,
    ) -> Result<(ToolInvocationResponse, Option<PatchReceipt>)> {
        let definition = self.definition(&request.tool_id)?.clone();
        let started = now_millis();
        if let Err(error) = self.policy.authorize(&definition, request.approval_granted) {
            return Ok((
                failed_response(
                    invocation_id,
                    request,
                    &definition,
                    started,
                    FailureClass::Permission,
                    "permission_denied",
                    error.to_string(),
                ),
                None,
            ));
        }
        let path = input_path(&request.input, "path")?;
        let expected_hash = input_string(&request.input, "expected_content_hash")?;
        let replacement = input_string(&request.input, "replacement_content")?;
        match self.patcher.apply(&path, &expected_hash, &replacement) {
            Ok(receipt) => {
                let output = json!({
                    "path": receipt.relative_path,
                    "before_hash": receipt.before_hash,
                    "after_hash": receipt.after_hash,
                    "diff": receipt.diff,
                    "dry_run_validated": true,
                });
                Ok((
                    completed_response(
                        invocation_id,
                        request,
                        &definition,
                        started,
                        output,
                        false,
                        "patch applied atomically",
                    ),
                    Some(receipt),
                ))
            }
            Err(error) => Ok((
                failed_response(
                    invocation_id,
                    request,
                    &definition,
                    started,
                    FailureClass::Validation,
                    "patch_failed",
                    error.to_string(),
                ),
                None,
            )),
        }
    }

    pub fn rollback_patch(&self, receipt: &PatchReceipt) -> Result<()> {
        self.patcher.rollback(receipt)
    }

    pub fn preview_patch(
        &self,
        relative_path: &Path,
        expected_content_hash: &str,
        replacement_content: &str,
    ) -> Result<PatchPreview> {
        self.patcher
            .preview(relative_path, expected_content_hash, replacement_content)
    }

    pub fn invoke_command(
        &self,
        invocation_id: InvocationId,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse> {
        let definition = self.definition(&request.tool_id)?.clone();
        let started = now_millis();
        if let Err(error) = self.policy.authorize(&definition, request.approval_granted) {
            return Ok(failed_response(
                invocation_id,
                request,
                &definition,
                started,
                FailureClass::Permission,
                "permission_denied",
                error.to_string(),
            ));
        }
        let (program, args) = match request.tool_id.as_str() {
            "git.status" => (
                "git".to_owned(),
                vec!["status".to_owned(), "--short".to_owned()],
            ),
            "git.diff" => ("git".to_owned(), vec!["diff".to_owned(), "--".to_owned()]),
            _ => (
                input_string(&request.input, "program")?,
                input_strings(&request.input, "args")?,
            ),
        };
        let working_directory = request
            .input
            .get("working_directory")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let timeout_millis = request
            .timeout_millis
            .unwrap_or(definition.default_timeout_millis)
            .min(60 * 60 * 1000);
        let command = SafeCommandRequest {
            program,
            args,
            working_directory,
            timeout_millis,
            max_output_bytes: usize::try_from(definition.max_output_bytes).unwrap_or(1_000_000),
        };
        let token = CancellationToken::default();
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(invocation_id.0.clone(), token.clone());
        let result = self.command_runner.run(&command, &token);
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(invocation_id.as_str());
        match result {
            Ok(output) => {
                let status = if output.cancelled {
                    InvocationStatus::Cancelled
                } else if output.status_code == 0 && !output.timed_out {
                    InvocationStatus::Completed
                } else {
                    InvocationStatus::Failed
                };
                let summary = if output.cancelled {
                    "command cancelled"
                } else if output.timed_out {
                    "command timed out"
                } else if output.status_code == 0 {
                    "command completed"
                } else {
                    "command exited unsuccessfully"
                };
                Ok(response_with_status(
                    invocation_id,
                    request,
                    &definition,
                    started,
                    serde_json::to_value(&output)?,
                    output.output_truncated,
                    summary,
                    status,
                    if status == InvocationStatus::Failed {
                        Some(FailureClass::Tool)
                    } else {
                        None
                    },
                    if output.timed_out {
                        Some(ErrorCode::new("command_timeout"))
                    } else if output.cancelled {
                        Some(ErrorCode::new("command_cancelled"))
                    } else if output.status_code != 0 {
                        Some(ErrorCode::new("command_failed"))
                    } else {
                        None
                    },
                ))
            }
            Err(error) => Ok(failed_response(
                invocation_id,
                request,
                &definition,
                started,
                FailureClass::Tool,
                "command_start_failed",
                error.to_string(),
            )),
        }
    }

    pub fn cancel(&self, invocation_id: &str) -> bool {
        let token = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(invocation_id)
            .cloned();
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }

    fn invoke_read_tool(
        &self,
        invocation_id: InvocationId,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse> {
        let definition = self.definition(&request.tool_id)?.clone();
        let started = now_millis();
        if let Err(error) = self.policy.authorize(&definition, request.approval_granted) {
            return Ok(failed_response(
                invocation_id,
                request,
                &definition,
                started,
                FailureClass::Permission,
                "permission_denied",
                error.to_string(),
            ));
        }
        let output = match request.tool_id.as_str() {
            "workspace.read_file" => {
                let path = input_path(&request.input, "path")?;
                let content = self.guard.read_text(
                    &path,
                    usize::try_from(definition.max_output_bytes).unwrap_or(1_000_000),
                )?;
                json!({"path": path, "content": content, "content_hash": blake3::hash(content.as_bytes()).to_hex().to_string()})
            }
            "workspace.list_directory" => {
                let path = request
                    .input
                    .get("path")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                let entries = self.guard.list_directory(&path, 500)?;
                json!({"path": path, "entries": entries})
            }
            "workspace.search_exact" => {
                let query = input_string(&request.input, "query")?;
                let path = request
                    .input
                    .get("path")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                let max_matches = request
                    .input
                    .get("max_matches")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(100);
                let matches = self
                    .guard
                    .search_exact(&query, &path, max_matches, 2_000_000)?;
                json!({"query": query, "matches": matches})
            }
            _ => return Err(anyhow!("unknown tool '{}'", request.tool_id)),
        };
        Ok(completed_response(
            invocation_id,
            request,
            &definition,
            started,
            output,
            false,
            "read-only tool completed",
        ))
    }

    fn definition(&self, tool_id: &str) -> Result<&ToolDefinition> {
        self.definitions
            .iter()
            .find(|definition| definition.tool_id == tool_id)
            .ok_or_else(|| anyhow!("unknown tool '{tool_id}'"))
    }
}

fn builtin_definitions(default_timeout_millis: u64, max_output_bytes: u64) -> Vec<ToolDefinition> {
    vec![
        definition(
            "workspace.read_file",
            "Read one UTF-8 file confined to the workspace.",
            vec![PermissionScope::WorkspaceRead],
            ToolEffect::ReadOnly,
            false,
            default_timeout_millis,
            max_output_bytes,
        ),
        definition(
            "workspace.list_directory",
            "List metadata for one workspace directory.",
            vec![PermissionScope::WorkspaceRead],
            ToolEffect::ReadOnly,
            false,
            default_timeout_millis,
            max_output_bytes,
        ),
        definition(
            "workspace.search_exact",
            "Search exact text in workspace files while respecting ignore rules.",
            vec![PermissionScope::WorkspaceRead],
            ToolEffect::ReadOnly,
            false,
            default_timeout_millis,
            max_output_bytes,
        ),
        definition(
            "workspace.apply_patch",
            "Atomically replace one file after validating its expected content hash.",
            vec![PermissionScope::WorkspaceWrite],
            ToolEffect::WorkspaceMutation,
            false,
            default_timeout_millis,
            max_output_bytes,
        ),
        definition(
            "process.run",
            "Run an allowlisted process inside the workspace with timeout and bounded output.",
            vec![PermissionScope::ProcessExecute],
            ToolEffect::Process,
            true,
            default_timeout_millis,
            max_output_bytes,
        ),
        definition(
            "git.status",
            "Read concise Git working tree status.",
            vec![PermissionScope::GitRead],
            ToolEffect::ReadOnly,
            true,
            default_timeout_millis,
            max_output_bytes,
        ),
        definition(
            "git.diff",
            "Read the current Git diff.",
            vec![PermissionScope::GitRead],
            ToolEffect::ReadOnly,
            true,
            default_timeout_millis,
            max_output_bytes,
        ),
    ]
}

fn definition(
    tool_id: &str,
    description: &str,
    permissions: Vec<PermissionScope>,
    effect: ToolEffect,
    cancellation: bool,
    default_timeout_millis: u64,
    max_output_bytes: u64,
) -> ToolDefinition {
    ToolDefinition {
        tool_id: tool_id.to_owned(),
        version: "1.0.0".to_owned(),
        description: description.to_owned(),
        input_schema: input_schema(tool_id),
        output_schema: output_schema(tool_id),
        required_permissions: permissions,
        default_timeout_millis,
        max_output_bytes,
        supports_cancellation: cancellation,
        verifier_hook: match tool_id {
            "workspace.apply_patch" => Some("execution.patch_verifier".to_owned()),
            "process.run" => Some("execution.exit_status".to_owned()),
            _ => None,
        },
        effect,
    }
}

fn input_schema(tool_id: &str) -> Value {
    match tool_id {
        "workspace.read_file" => json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}},
            "additionalProperties": false
        }),
        "workspace.list_directory" => json!({
            "type": "object",
            "properties": {"path": {"type": "string", "default": "."}},
            "additionalProperties": false
        }),
        "workspace.search_exact" => json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {"type": "string"},
                "path": {"type": "string", "default": "."},
                "max_matches": {"type": "integer", "minimum": 1, "maximum": 10000}
            },
            "additionalProperties": false
        }),
        "workspace.apply_patch" => json!({
            "type": "object",
            "required": ["path", "expected_content_hash", "replacement_content"],
            "properties": {
                "path": {"type": "string"},
                "expected_content_hash": {"type": "string"},
                "replacement_content": {"type": "string"}
            },
            "additionalProperties": false
        }),
        "process.run" => json!({
            "type": "object",
            "required": ["program"],
            "properties": {
                "program": {"type": "string"},
                "args": {"type": "array", "items": {"type": "string"}},
                "working_directory": {"type": "string", "default": "."}
            },
            "additionalProperties": false
        }),
        "git.status" | "git.diff" => json!({"type": "object", "additionalProperties": false}),
        _ => json!({"type": "object"}),
    }
}

fn output_schema(tool_id: &str) -> Value {
    match tool_id {
        "workspace.read_file" => json!({
            "type": "object",
            "required": ["path", "content", "content_hash"]
        }),
        "workspace.list_directory" => json!({
            "type": "object",
            "required": ["path", "entries"]
        }),
        "workspace.search_exact" => json!({
            "type": "object",
            "required": ["query", "matches"]
        }),
        "workspace.apply_patch" => json!({
            "type": "object",
            "required": ["path", "before_hash", "after_hash", "diff"]
        }),
        "process.run" | "git.status" | "git.diff" => json!({
            "type": "object",
            "required": ["status_code", "stdout", "stderr", "timed_out", "cancelled", "output_truncated"]
        }),
        _ => json!({"type": "object"}),
    }
}

fn completed_response(
    invocation_id: InvocationId,
    request: ToolInvocationRequest,
    definition: &ToolDefinition,
    started: u128,
    output: Value,
    output_truncated: bool,
    summary: &str,
) -> ToolInvocationResponse {
    response_with_status(
        invocation_id,
        request,
        definition,
        started,
        output,
        output_truncated,
        summary,
        InvocationStatus::Completed,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn response_with_status(
    invocation_id: InvocationId,
    request: ToolInvocationRequest,
    definition: &ToolDefinition,
    started: u128,
    output: Value,
    output_truncated: bool,
    summary: &str,
    status: InvocationStatus,
    failure_class: Option<FailureClass>,
    error_code: Option<ErrorCode>,
) -> ToolInvocationResponse {
    let replay_key = replay_key(definition, &request.input);
    ToolInvocationResponse {
        invocation: ToolInvocationRecord {
            invocation_id,
            run_id: request.run_id,
            stage_id: None,
            tool_name: definition.tool_id.clone(),
            tool_version: definition.version.clone(),
            required_permissions: definition.required_permissions.clone(),
            effect: definition.effect,
            status,
            started_at_epoch_millis: started,
            finished_at_epoch_millis: Some(now_millis()),
            arguments: request.input,
            output: output.clone(),
            output_truncated,
            timeout_millis: request
                .timeout_millis
                .or(Some(definition.default_timeout_millis)),
            replay_key: Some(replay_key),
            result_summary: Some(summary.to_owned()),
            failure_class,
            error_code,
        },
        output,
    }
}

fn failed_response(
    invocation_id: InvocationId,
    request: ToolInvocationRequest,
    definition: &ToolDefinition,
    started: u128,
    failure_class: FailureClass,
    error_code: &str,
    message: String,
) -> ToolInvocationResponse {
    response_with_status(
        invocation_id,
        request,
        definition,
        started,
        json!({"error": message}),
        false,
        "tool invocation failed",
        InvocationStatus::Failed,
        Some(failure_class),
        Some(ErrorCode::new(error_code)),
    )
}

fn replay_key(definition: &ToolDefinition, input: &Value) -> String {
    let payload = serde_json::to_vec(&json!({
        "tool_id": definition.tool_id,
        "version": definition.version,
        "input": input,
    }))
    .unwrap_or_default();
    blake3::hash(&payload).to_hex().to_string()
}

fn input_string(input: &Value, field: &str) -> Result<String> {
    input
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("input field '{field}' must be a string"))
}

fn input_strings(input: &Value, field: &str) -> Result<Vec<String>> {
    let Some(value) = input.get(field) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| anyhow!("input field '{field}' must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("input field '{field}' must contain strings"))
        })
        .collect()
}

fn input_path(input: &Value, field: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(input_string(input, field)?))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::{ToolPolicy, ToolRuntime, content_hash};
    use everything_domain::{InvocationId, RunId, ToolInvocationRequest};
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn workspace(label: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-tools-{label}-{suffix}"));
        std::fs::create_dir_all(&root).expect("workspace");
        root
    }

    #[test]
    fn blocks_workspace_escape_and_requires_write_approval() {
        let root = workspace("policy");
        std::fs::write(root.join("demo.txt"), "before").expect("file");
        let runtime = ToolRuntime::new(&root, ToolPolicy::default(), vec!["git".to_owned()])
            .expect("runtime");
        let response = runtime
            .invoke(
                InvocationId::new("invocation-1"),
                ToolInvocationRequest {
                    run_id: RunId::new("run-1"),
                    tool_id: "workspace.apply_patch".to_owned(),
                    input: json!({
                        "path": "demo.txt",
                        "expected_content_hash": content_hash(b"before"),
                        "replacement_content": "after",
                    }),
                    approval_granted: false,
                    timeout_millis: None,
                },
            )
            .expect("policy response");
        assert_eq!(
            response.invocation.status,
            everything_domain::InvocationStatus::Failed
        );
        assert_eq!(
            std::fs::read_to_string(root.join("demo.txt")).unwrap(),
            "before"
        );

        let escaped = runtime.invoke(
            InvocationId::new("invocation-2"),
            ToolInvocationRequest {
                run_id: RunId::new("run-1"),
                tool_id: "workspace.read_file".to_owned(),
                input: json!({"path": "../secret.txt"}),
                approval_granted: false,
                timeout_millis: None,
            },
        );
        assert!(escaped.is_err());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn protects_runtime_and_git_metadata_from_workspace_tools() {
        let root = workspace("protected-metadata");
        std::fs::create_dir_all(root.join(".git")).expect("git dir");
        std::fs::create_dir_all(root.join(".everything")).expect("runtime dir");
        std::fs::write(root.join(".git/config"), "secret").expect("git config");
        std::fs::write(root.join(".everything/state"), "secret").expect("state");
        std::fs::write(
            root.join("everything.toml"),
            "[model]
",
        )
        .expect("policy");
        let runtime =
            ToolRuntime::new(&root, ToolPolicy::default(), Vec::<String>::new()).expect("runtime");

        for path in [".git/config", ".everything/state"] {
            let result = runtime.invoke(
                InvocationId::new(format!("read-{path}")),
                ToolInvocationRequest {
                    run_id: RunId::new("run-1"),
                    tool_id: "workspace.read_file".to_owned(),
                    input: json!({"path": path}),
                    approval_granted: false,
                    timeout_millis: None,
                },
            );
            assert!(result.is_err(), "protected path should fail: {path}");
        }

        let policy_patch = runtime.invoke(
            InvocationId::new("patch-policy"),
            ToolInvocationRequest {
                run_id: RunId::new("run-1"),
                tool_id: "workspace.apply_patch".to_owned(),
                input: json!({
                    "path": "everything.toml",
                    "expected_content_hash": content_hash(b"[model]\n"),
                    "replacement_content": "[model]\nbackend = 'loopback'\n",
                }),
                approval_granted: true,
                timeout_millis: None,
            },
        );
        let policy_patch = policy_patch.expect("protected patch response");
        assert_eq!(
            policy_patch.invocation.status,
            everything_domain::InvocationStatus::Failed
        );
        assert_eq!(
            std::fs::read_to_string(root.join("everything.toml")).expect("policy unchanged"),
            "[model]
"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn applies_and_rolls_back_hash_guarded_patch() {
        let root = workspace("patch");
        std::fs::write(root.join("demo.txt"), "before\n").expect("file");
        let runtime =
            ToolRuntime::new(&root, ToolPolicy::default(), Vec::<String>::new()).expect("runtime");
        let (response, receipt) = runtime
            .invoke_patch_with_receipt(
                InvocationId::new("invocation-1"),
                ToolInvocationRequest {
                    run_id: RunId::new("run-1"),
                    tool_id: "workspace.apply_patch".to_owned(),
                    input: json!({
                        "path": "demo.txt",
                        "expected_content_hash": content_hash(b"before\n"),
                        "replacement_content": "after\n",
                    }),
                    approval_granted: true,
                    timeout_millis: None,
                },
            )
            .expect("patch");
        assert_eq!(
            response.invocation.status,
            everything_domain::InvocationStatus::Completed
        );
        let receipt = receipt.expect("receipt");
        runtime.rollback_patch(&receipt).expect("rollback");
        assert_eq!(
            std::fs::read_to_string(root.join("demo.txt")).unwrap(),
            "before\n"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
