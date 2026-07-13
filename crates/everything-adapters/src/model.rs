use crate::ports::{ModelAdapter, ModelCompletion, ModelPrompt};
use anyhow::{Context, Result};
use everything_domain::{
    DiscoveredModel, ModelCapabilityProfile, ModelHealth, ModelHealthStatus, ModelQualityTier,
};
use regex::Regex;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct LocalModelAdapter {
    model_name: String,
}

impl LocalModelAdapter {
    pub fn new(model_name: impl Into<String>) -> Self {
        Self {
            model_name: model_name.into(),
        }
    }
}

impl ModelAdapter for LocalModelAdapter {
    fn name(&self) -> &str {
        &self.model_name
    }

    fn capability_profile(&self) -> ModelCapabilityProfile {
        ModelCapabilityProfile::conservative_loopback(self.model_name.clone())
    }

    fn discover_models(&self) -> Result<Vec<DiscoveredModel>> {
        Ok(vec![DiscoveredModel {
            provider: "loopback".to_owned(),
            model_name: self.model_name.clone(),
            installed: true,
            configured: true,
            profile: self.capability_profile(),
        }])
    }

    fn complete(&self, prompt: ModelPrompt) -> Result<ModelCompletion> {
        let mut content = String::from("Architecture Snapshot\n");
        content.push_str("- Runtime style: native-first, modular, adapter-driven.\n");
        content.push_str(
            "- Model stance: keep the 9B model on bounded work and verify every mutating step.\n",
        );
        content.push_str("- Objective:\n");
        content.push_str(&format!("  {}\n", prompt.user_instruction));

        if !prompt.context.is_empty() {
            content.push_str("- Context:\n");
            for (key, value) in prompt.context {
                content.push_str(&format!("  - {key}: {value}\n"));
            }
        }

        content.push_str("- Recommended next steps:\n");
        content.push_str(
            "  1. Route repository understanding through the graph before broad reads.\n",
        );
        content.push_str("  2. Keep orchestration deterministic and stateful.\n");
        content.push_str("  3. Add verifier passes before shell writes or code mutation.\n");
        content.push_str("  4. Preserve adapter boundaries so the real local model backend can replace this stub cleanly.\n");

        Ok(ModelCompletion {
            content: content.trim_end().to_owned(),
            model_name: self.model_name.clone(),
            is_fallback: false,
            fallback_reason: None,
            capability_profile: self.capability_profile(),
        })
    }

    fn health_check(&self) -> Result<ModelHealth> {
        Ok(ModelHealth {
            status: ModelHealthStatus::Healthy,
            available: true,
            adapter: self.model_name.clone(),
            detail: "loopback adapter ready".to_owned(),
            primary_available: true,
            fallback_available: false,
            fallback_active: false,
        })
    }
}

#[derive(Debug, Clone)]
pub struct OllamaModelAdapter {
    model_name: String,
    binary: String,
    keep_alive: String,
    hide_thinking: bool,
    timeout_millis: u64,
    max_output_bytes: usize,
    context_window_override: Option<u32>,
    safe_context_override: Option<u32>,
}

impl OllamaModelAdapter {
    pub fn new(
        model_name: impl Into<String>,
        binary: impl Into<String>,
        keep_alive: impl Into<String>,
        hide_thinking: bool,
    ) -> Self {
        Self {
            model_name: model_name.into(),
            binary: binary.into(),
            keep_alive: keep_alive.into(),
            hide_thinking,
            timeout_millis: 180_000,
            max_output_bytes: 4_000_000,
            context_window_override: None,
            safe_context_override: None,
        }
    }

    pub fn with_limits(mut self, timeout_millis: u64, max_output_bytes: u64) -> Self {
        self.timeout_millis = timeout_millis.clamp(1_000, 60 * 60 * 1_000);
        self.max_output_bytes = max_output_bytes.clamp(1_024, 100_000_000) as usize;
        self
    }

    pub fn with_context_override(
        mut self,
        context_window_tokens: Option<u32>,
        safe_context_tokens: Option<u32>,
    ) -> Self {
        self.context_window_override = context_window_tokens;
        self.safe_context_override = safe_context_tokens;
        self
    }
}

impl ModelAdapter for OllamaModelAdapter {
    fn name(&self) -> &str {
        &self.model_name
    }

    fn capability_profile(&self) -> ModelCapabilityProfile {
        let mut profile = infer_ollama_profile(&self.model_name);
        if let Some(context_window_tokens) = self.context_window_override {
            profile.context_window_tokens = context_window_tokens.max(1_024);
        }
        if let Some(safe_context_tokens) = self.safe_context_override {
            profile.safe_context_tokens = safe_context_tokens
                .max(1_024)
                .min(profile.context_window_tokens);
        } else {
            profile.safe_context_tokens = profile
                .safe_context_tokens
                .min(profile.context_window_tokens);
        }
        profile
    }

    fn discover_models(&self) -> Result<Vec<DiscoveredModel>> {
        let mut command = Command::new(&self.binary);
        command.arg("list");
        let output = run_bounded(
            command,
            self.timeout_millis.min(30_000),
            self.max_output_bytes.min(1_000_000),
        )
        .with_context(|| format!("failed to execute {}", self.binary))?;
        if output.timed_out {
            anyhow::bail!("ollama model discovery timed out");
        }
        if !output.status.success() {
            anyhow::bail!("ollama model discovery failed: {}", output.stderr.trim());
        }

        let mut names = output
            .stdout
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !names.iter().any(|name| name == &self.model_name) {
            names.push(self.model_name.clone());
        }
        names.sort();
        names.dedup();
        Ok(names
            .into_iter()
            .map(|model_name| {
                let installed = output
                    .stdout
                    .lines()
                    .skip(1)
                    .filter_map(|line| line.split_whitespace().next())
                    .any(|name| name == model_name.as_str());
                let profile = if model_name == self.model_name {
                    self.capability_profile()
                } else {
                    infer_ollama_profile(&model_name)
                };
                DiscoveredModel {
                    provider: "ollama".to_owned(),
                    configured: model_name == self.model_name,
                    model_name,
                    installed,
                    profile,
                }
            })
            .collect())
    }

    fn complete(&self, prompt: ModelPrompt) -> Result<ModelCompletion> {
        let combined_prompt = format!(
            "{}\n\nObjective:\n{}\n\nContext:\n{}",
            prompt.system_instruction,
            prompt.user_instruction,
            prompt
                .context
                .into_iter()
                .map(|(key, value)| format!("- {key}: {value}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let mut command = Command::new(&self.binary);
        command
            .arg("run")
            .arg(&self.model_name)
            .arg(combined_prompt);
        command.arg("--keepalive").arg(&self.keep_alive);
        command.arg("--nowordwrap");
        if self.hide_thinking {
            command.arg("--hidethinking");
        }

        let output = run_bounded(command, self.timeout_millis, self.max_output_bytes)
            .with_context(|| format!("failed to execute {}", self.binary))?;

        if output.timed_out {
            anyhow::bail!("ollama run timed out after {} ms", self.timeout_millis);
        }
        if !output.status.success() {
            anyhow::bail!("ollama run failed: {}", output.stderr.trim());
        }
        if output.truncated {
            anyhow::bail!(
                "ollama output exceeded configured {} byte limit",
                self.max_output_bytes
            );
        }

        let content = strip_ansi(output.stdout.trim());

        Ok(ModelCompletion {
            content,
            model_name: self.model_name.clone(),
            is_fallback: false,
            fallback_reason: None,
            capability_profile: self.capability_profile(),
        })
    }

    fn health_check(&self) -> Result<ModelHealth> {
        let mut command = Command::new(&self.binary);
        command.arg("list");
        let output = match run_bounded(
            command,
            self.timeout_millis.min(30_000),
            self.max_output_bytes.min(1_000_000),
        )
        .with_context(|| format!("failed to execute {}", self.binary))
        {
            Ok(output) => output,
            Err(error) => {
                return Ok(ModelHealth {
                    status: ModelHealthStatus::Unavailable,
                    available: false,
                    adapter: self.model_name.clone(),
                    detail: error.to_string(),
                    primary_available: false,
                    fallback_available: false,
                    fallback_active: false,
                });
            }
        };

        if output.timed_out || !output.status.success() {
            return Ok(ModelHealth {
                status: ModelHealthStatus::Unavailable,
                available: false,
                adapter: self.model_name.clone(),
                detail: if output.timed_out {
                    format!(
                        "ollama health check timed out after {} ms",
                        self.timeout_millis.min(30_000)
                    )
                } else {
                    output.stderr.trim().to_owned()
                },
                primary_available: false,
                fallback_available: false,
                fallback_active: false,
            });
        }

        let available = output
            .stdout
            .lines()
            .any(|line| line.contains(&self.model_name));
        let detail = if available {
            format!(
                "ollama reachable and model '{}' is present",
                self.model_name
            )
        } else {
            format!(
                "ollama reachable but model '{}' is not installed",
                self.model_name
            )
        };

        Ok(ModelHealth {
            status: if available {
                ModelHealthStatus::Healthy
            } else {
                ModelHealthStatus::Unavailable
            },
            available,
            adapter: self.model_name.clone(),
            detail,
            primary_available: available,
            fallback_available: false,
            fallback_active: false,
        })
    }
}

pub struct ResilientModelAdapter {
    primary: Box<dyn ModelAdapter>,
    fallback: Box<dyn ModelAdapter>,
}

impl ResilientModelAdapter {
    pub fn new(primary: Box<dyn ModelAdapter>, fallback: Box<dyn ModelAdapter>) -> Self {
        Self { primary, fallback }
    }
}

impl ModelAdapter for ResilientModelAdapter {
    fn name(&self) -> &str {
        self.primary.name()
    }

    fn capability_profile(&self) -> ModelCapabilityProfile {
        self.primary.capability_profile()
    }

    fn discover_models(&self) -> Result<Vec<DiscoveredModel>> {
        let mut models = self.primary.discover_models().unwrap_or_default();
        models.extend(self.fallback.discover_models().unwrap_or_default());
        models.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.model_name.cmp(&right.model_name))
        });
        models.dedup_by(|left, right| {
            left.provider == right.provider && left.model_name == right.model_name
        });
        Ok(models)
    }

    fn complete(&self, prompt: ModelPrompt) -> Result<ModelCompletion> {
        match self.primary.complete(prompt.clone()) {
            Ok(result) => Ok(result),
            Err(primary_error) => {
                let reason = primary_error.to_string();
                let mut completion = self.fallback.complete(prompt).with_context(|| {
                    format!(
                        "primary model '{}' failed ({reason}); fallback '{}' also failed",
                        self.primary.name(),
                        self.fallback.name()
                    )
                })?;
                completion.is_fallback = true;
                completion.fallback_reason = Some(reason);
                Ok(completion)
            }
        }
    }

    fn health_check(&self) -> Result<ModelHealth> {
        let primary = self
            .primary
            .health_check()
            .unwrap_or_else(|error| ModelHealth {
                status: ModelHealthStatus::Unavailable,
                available: false,
                adapter: self.primary.name().to_owned(),
                detail: error.to_string(),
                primary_available: false,
                fallback_available: false,
                fallback_active: false,
            });
        if primary.available {
            return Ok(ModelHealth {
                status: ModelHealthStatus::Healthy,
                available: true,
                adapter: self.primary.name().to_owned(),
                detail: primary.detail,
                primary_available: true,
                fallback_available: false,
                fallback_active: false,
            });
        }

        let fallback = self
            .fallback
            .health_check()
            .unwrap_or_else(|error| ModelHealth {
                status: ModelHealthStatus::Unavailable,
                available: false,
                adapter: self.fallback.name().to_owned(),
                detail: error.to_string(),
                primary_available: false,
                fallback_available: false,
                fallback_active: false,
            });
        Ok(ModelHealth {
            status: if fallback.available {
                ModelHealthStatus::Degraded
            } else {
                ModelHealthStatus::Unavailable
            },
            available: fallback.available,
            adapter: if fallback.available {
                self.fallback.name().to_owned()
            } else {
                self.primary.name().to_owned()
            },
            detail: if fallback.available {
                format!(
                    "primary '{}' unavailable: {}; reported fallback '{}' is active",
                    self.primary.name(),
                    primary.detail,
                    self.fallback.name()
                )
            } else {
                format!(
                    "primary '{}' unavailable: {}; fallback '{}' unavailable: {}",
                    self.primary.name(),
                    primary.detail,
                    self.fallback.name(),
                    fallback.detail
                )
            },
            primary_available: false,
            fallback_available: fallback.available,
            fallback_active: fallback.available,
        })
    }
}

struct BoundedProcessOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
    timed_out: bool,
    truncated: bool,
}

fn run_bounded(
    mut command: Command,
    timeout_millis: u64,
    max_output_bytes: usize,
) -> Result<BoundedProcessOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().context("capture model stdout")?;
    let stderr = child.stderr.take().context("capture model stderr")?;
    let limit = max_output_bytes.max(1);
    let stdout_thread = std::thread::spawn(move || read_limited(stdout, limit));
    let stderr_thread = std::thread::spawn(move || read_limited(stderr, limit));
    let started = Instant::now();
    let timeout = Duration::from_millis(timeout_millis.max(1));

    let (status, timed_out) = loop {
        if started.elapsed() >= timeout {
            terminate_process_tree(&mut child);
            break (child.wait()?, true);
        }
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (stdout, stdout_truncated) = stdout_thread.join().unwrap_or_else(|_| (Vec::new(), true));
    let (stderr, stderr_truncated) = stderr_thread.join().unwrap_or_else(|_| (Vec::new(), true));
    Ok(BoundedProcessOutput {
        status,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        timed_out,
        truncated: stdout_truncated || stderr_truncated,
    })
}

fn read_limited(mut reader: impl Read, limit: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; 8_192];
    let mut truncated = false;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let remaining = limit.saturating_sub(kept.len());
        if remaining > 0 {
            kept.extend_from_slice(&buffer[..count.min(remaining)]);
        }
        if count > remaining {
            truncated = true;
        }
    }
    (kept, truncated)
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_process_tree(child: &mut Child) {
    let _ = Command::new("kill")
        .args(["-TERM", &format!("-{}", child.id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    std::thread::sleep(Duration::from_millis(50));
    let _ = child.kill();
}

#[cfg(windows)]
fn terminate_process_tree(child: &mut Child) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &child.id().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

#[cfg(not(any(unix, windows)))]
fn terminate_process_tree(child: &mut Child) {
    let _ = child.kill();
}

fn infer_ollama_profile(model_name: &str) -> ModelCapabilityProfile {
    let lowered = model_name.to_ascii_lowercase();
    let (quality_tier, context_window_tokens, safe_context_tokens, memory_estimate_mb) =
        if lowered.contains("70b") || lowered.contains("72b") {
            (ModelQualityTier::Large, 131_072, 48_000, Some(48_000))
        } else if lowered.contains("30b") || lowered.contains("32b") || lowered.contains("34b") {
            (ModelQualityTier::Large, 65_536, 32_000, Some(24_000))
        } else if lowered.contains("13b") || lowered.contains("14b") {
            (ModelQualityTier::Medium, 65_536, 24_000, Some(12_000))
        } else if lowered.contains("7b") || lowered.contains("8b") || lowered.contains("9b") {
            (ModelQualityTier::Small, 32_768, 12_000, Some(8_000))
        } else {
            (ModelQualityTier::Small, 16_384, 8_000, None)
        };
    let coder = lowered.contains("coder") || lowered.contains("code");
    ModelCapabilityProfile {
        provider: "ollama".to_owned(),
        model_name: model_name.to_owned(),
        quality_tier,
        context_window_tokens,
        safe_context_tokens,
        coding_suitability: if coder { 0.85 } else { 0.6 },
        structured_output_reliability: if coder { 0.72 } else { 0.58 },
        tool_calling_reliability: if lowered.contains("qwen") { 0.68 } else { 0.5 },
        estimated_tokens_per_second: None,
        memory_estimate_mb,
        recommended_task_classes: if coder {
            vec![
                "repository_investigation".to_owned(),
                "narrow_code_edit".to_owned(),
                "debugging".to_owned(),
            ]
        } else {
            vec!["repository_investigation".to_owned(), "planning".to_owned()]
        },
    }
}

fn strip_ansi(input: &str) -> String {
    let regex = Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("valid ansi regex");
    regex.replace_all(input, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        LocalModelAdapter, OllamaModelAdapter, ResilientModelAdapter, infer_ollama_profile,
    };
    use crate::ports::{ModelAdapter, ModelCompletion, ModelPrompt};
    use anyhow::{Result, anyhow};
    use everything_domain::{
        DiscoveredModel, ModelCapabilityProfile, ModelHealth, ModelHealthStatus, ModelQualityTier,
    };
    use std::collections::BTreeMap;

    struct StubModel {
        name: &'static str,
        completion: Result<&'static str, &'static str>,
        healthy: bool,
    }

    impl ModelAdapter for StubModel {
        fn name(&self) -> &str {
            self.name
        }

        fn capability_profile(&self) -> ModelCapabilityProfile {
            ModelCapabilityProfile::conservative_loopback(self.name)
        }

        fn discover_models(&self) -> Result<Vec<DiscoveredModel>> {
            Ok(vec![DiscoveredModel {
                provider: "test".to_owned(),
                model_name: self.name.to_owned(),
                installed: self.healthy,
                configured: true,
                profile: self.capability_profile(),
            }])
        }

        fn complete(&self, _prompt: ModelPrompt) -> Result<ModelCompletion> {
            match self.completion {
                Ok(content) => Ok(ModelCompletion {
                    content: content.to_owned(),
                    model_name: self.name.to_owned(),
                    is_fallback: false,
                    fallback_reason: None,
                    capability_profile: self.capability_profile(),
                }),
                Err(message) => Err(anyhow!(message)),
            }
        }

        fn health_check(&self) -> Result<ModelHealth> {
            Ok(ModelHealth {
                status: if self.healthy {
                    ModelHealthStatus::Healthy
                } else {
                    ModelHealthStatus::Unavailable
                },
                available: self.healthy,
                adapter: self.name.to_owned(),
                detail: if self.healthy {
                    "ready".to_owned()
                } else {
                    "unavailable".to_owned()
                },
                primary_available: self.healthy,
                fallback_available: false,
                fallback_active: false,
            })
        }
    }

    fn prompt() -> ModelPrompt {
        ModelPrompt {
            system_instruction: "system".to_owned(),
            user_instruction: "objective".to_owned(),
            context: BTreeMap::new(),
        }
    }

    #[test]
    fn primary_success_is_not_marked_as_fallback() {
        let adapter = ResilientModelAdapter::new(
            Box::new(StubModel {
                name: "primary",
                completion: Ok("primary output"),
                healthy: true,
            }),
            Box::new(StubModel {
                name: "fallback",
                completion: Ok("fallback output"),
                healthy: true,
            }),
        );

        let completion = adapter.complete(prompt()).expect("primary completion");
        assert_eq!(completion.model_name, "primary");
        assert!(!completion.is_fallback);
        assert_eq!(completion.fallback_reason, None);
    }

    #[test]
    fn primary_failure_is_reported_on_fallback_completion() {
        let adapter = ResilientModelAdapter::new(
            Box::new(StubModel {
                name: "primary",
                completion: Err("primary exploded"),
                healthy: false,
            }),
            Box::new(StubModel {
                name: "fallback",
                completion: Ok("fallback output"),
                healthy: true,
            }),
        );

        let completion = adapter.complete(prompt()).expect("fallback completion");
        assert_eq!(completion.model_name, "fallback");
        assert!(completion.is_fallback);
        assert_eq!(
            completion.fallback_reason.as_deref(),
            Some("primary exploded")
        );
    }

    #[test]
    fn health_identifies_active_fallback() {
        let adapter = ResilientModelAdapter::new(
            Box::new(StubModel {
                name: "primary",
                completion: Err("primary exploded"),
                healthy: false,
            }),
            Box::new(StubModel {
                name: "fallback",
                completion: Ok("fallback output"),
                healthy: true,
            }),
        );

        let health = adapter.health_check().expect("health");
        assert!(health.available);
        assert!(!health.primary_available);
        assert!(health.fallback_available);
        assert!(health.fallback_active);
        assert_eq!(health.adapter, "fallback");
    }
    #[test]
    fn missing_ollama_binary_reports_fallback_health_instead_of_erroring() {
        let adapter = ResilientModelAdapter::new(
            Box::new(OllamaModelAdapter::new(
                "qwen2.5-coder:7b",
                "__everything_missing_ollama_binary__",
                "5m",
                true,
            )),
            Box::new(LocalModelAdapter::new("local-fallback")),
        );

        let health = adapter.health_check().expect("health report");
        assert_eq!(health.status, ModelHealthStatus::Degraded);
        assert!(health.available);
        assert!(!health.primary_available);
        assert!(health.fallback_available);
        assert!(health.fallback_active);
        assert!(health.detail.contains("failed to execute"));
    }

    #[test]
    fn infers_small_coder_profile_for_qwen_7b() {
        let profile = infer_ollama_profile("qwen2.5-coder:7b");
        assert_eq!(profile.quality_tier, ModelQualityTier::Small);
        assert_eq!(profile.safe_context_tokens, 12_000);
        assert!(profile.coding_suitability >= 0.8);
    }

    #[test]
    fn infers_large_profile_for_32b_models() {
        let profile = infer_ollama_profile("qwen2.5-coder:32b");
        assert_eq!(profile.quality_tier, ModelQualityTier::Large);
        assert_eq!(profile.safe_context_tokens, 32_000);
    }
}
