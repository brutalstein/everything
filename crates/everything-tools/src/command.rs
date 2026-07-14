use crate::workspace::WorkspaceGuard;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeCommandRequest {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    pub timeout_millis: u64,
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeCommandOutput {
    pub status_code: i32,
    pub termination_signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_millis: u128,
    pub timed_out: bool,
    pub cancelled: bool,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SandboxBackend {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Bubblewrap,
    #[cfg(target_os = "macos")]
    SandboxExec,
    GuardedProcess,
}

#[derive(Debug, Clone)]
pub struct SafeCommandRunner {
    guard: WorkspaceGuard,
    allowed_programs: BTreeSet<String>,
    trusted_workspace: bool,
    sandbox_backend: SandboxBackend,
}

impl SafeCommandRunner {
    pub fn new(guard: WorkspaceGuard, allowed_programs: impl IntoIterator<Item = String>) -> Self {
        Self::new_with_security(guard, allowed_programs, false, true)
    }

    pub fn new_with_security(
        guard: WorkspaceGuard,
        allowed_programs: impl IntoIterator<Item = String>,
        trusted_workspace: bool,
        os_sandbox_enabled: bool,
    ) -> Self {
        let sandbox_backend = detect_sandbox_backend(os_sandbox_enabled);
        Self {
            guard,
            allowed_programs: allowed_programs
                .into_iter()
                .map(|value| normalize_program(&value))
                .collect(),
            trusted_workspace,
            sandbox_backend,
        }
    }

    pub fn sandbox_description(&self) -> &'static str {
        match self.sandbox_backend {
            SandboxBackend::Bubblewrap => {
                "bubblewrap: filesystem writes limited to workspace; network isolated"
            }
            #[cfg(target_os = "macos")]
            SandboxBackend::SandboxExec => {
                "sandbox-exec: filesystem writes limited to workspace; network denied"
            }
            SandboxBackend::GuardedProcess => {
                "guarded process: allowlist, dangerous-program denylist, path validation"
            }
        }
    }

    pub fn run(
        &self,
        request: &SafeCommandRequest,
        cancellation: &CancellationToken,
    ) -> Result<SafeCommandOutput> {
        validate_program(
            &request.program,
            &request.args,
            &self.allowed_programs,
            self.trusted_workspace,
            self.sandbox_backend,
        )?;
        let working_directory = if request.working_directory.as_os_str().is_empty()
            || request.working_directory == Path::new(".")
        {
            self.guard.root().to_path_buf()
        } else {
            self.guard.resolve_existing(&request.working_directory)?
        };
        anyhow::ensure!(
            working_directory.is_dir(),
            "working directory is not a directory"
        );

        let tool_home = self.guard.root().join(".everything/tool-home");
        let tool_cache = self.guard.root().join(".everything/tool-cache");
        std::fs::create_dir_all(&tool_home)?;
        std::fs::create_dir_all(tool_cache.join("cargo"))?;
        std::fs::create_dir_all(tool_cache.join("npm"))?;
        std::fs::create_dir_all(tool_cache.join("pip"))?;
        std::fs::create_dir_all(tool_cache.join("xdg"))?;

        let mut command = self.build_command(request, &working_directory)?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();
        copy_safe_environment(&mut command);
        command.env("EVERYTHING_WORKSPACE", self.guard.root());
        command.env("EVERYTHING_TOOL_SANDBOX", self.sandbox_description());
        command.env("HOME", &tool_home);
        command.env("USERPROFILE", &tool_home);
        command.env("CARGO_HOME", tool_cache.join("cargo"));
        command.env("NPM_CONFIG_CACHE", tool_cache.join("npm"));
        command.env("PIP_CACHE_DIR", tool_cache.join("pip"));
        command.env("XDG_CACHE_HOME", tool_cache.join("xdg"));
        if let Some(rustup_home) = std::env::var_os("RUSTUP_HOME") {
            command.env("RUSTUP_HOME", rustup_home);
        } else if let Some(home) = platform_home() {
            command.env("RUSTUP_HOME", home.join(".rustup"));
        }
        configure_process_group(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to execute {}", request.program))?;
        let stdout = child.stdout.take().context("capture command stdout")?;
        let stderr = child.stderr.take().context("capture command stderr")?;
        let output_limit = request.max_output_bytes.max(1);
        let stdout_thread = std::thread::spawn(move || read_limited(stdout, output_limit));
        let stderr_thread = std::thread::spawn(move || read_limited(stderr, output_limit));

        let started = Instant::now();
        let timeout = Duration::from_millis(request.timeout_millis.max(1));
        let (status, timed_out, cancelled) = loop {
            if cancellation.is_cancelled() {
                terminate_process_tree(&mut child);
                break (child.wait().ok(), false, true);
            }
            if started.elapsed() >= timeout {
                terminate_process_tree(&mut child);
                break (child.wait().ok(), true, false);
            }
            if let Some(status) = child.try_wait()? {
                break (Some(status), false, false);
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let status_code = status.as_ref().and_then(ExitStatus::code).unwrap_or(-1);
        let termination_signal = status.as_ref().and_then(termination_signal);

        let (stdout, stdout_truncated) =
            stdout_thread.join().unwrap_or_else(|_| (Vec::new(), true));
        let (stderr, stderr_truncated) =
            stderr_thread.join().unwrap_or_else(|_| (Vec::new(), true));
        Ok(SafeCommandOutput {
            status_code,
            termination_signal,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            duration_millis: started.elapsed().as_millis(),
            timed_out,
            cancelled,
            output_truncated: stdout_truncated || stderr_truncated,
        })
    }

    fn build_command(
        &self,
        request: &SafeCommandRequest,
        working_directory: &Path,
    ) -> Result<Command> {
        match self.sandbox_backend {
            SandboxBackend::Bubblewrap => {
                let bwrap = find_program("bwrap")
                    .ok_or_else(|| anyhow!("bubblewrap disappeared after startup"))?;
                let mut command = Command::new(bwrap);
                command.args([
                    OsStr::new("--die-with-parent"),
                    OsStr::new("--new-session"),
                    OsStr::new("--unshare-net"),
                    OsStr::new("--unshare-ipc"),
                    OsStr::new("--unshare-uts"),
                    OsStr::new("--ro-bind"),
                    OsStr::new("/"),
                    OsStr::new("/"),
                    OsStr::new("--dev-bind"),
                    OsStr::new("/dev"),
                    OsStr::new("/dev"),
                    OsStr::new("--proc"),
                    OsStr::new("/proc"),
                    OsStr::new("--tmpfs"),
                    OsStr::new("/tmp"),
                    OsStr::new("--bind"),
                ]);
                command.arg(self.guard.root()).arg(self.guard.root());
                let policy_file = self.guard.root().join("everything.toml");
                if policy_file.is_file() {
                    command.arg("--ro-bind").arg(&policy_file).arg(&policy_file);
                }
                command.arg("--chdir").arg(working_directory);
                command.arg("--").arg(&request.program).args(&request.args);
                Ok(command)
            }
            #[cfg(target_os = "macos")]
            SandboxBackend::SandboxExec => {
                let sandbox = find_program("sandbox-exec")
                    .ok_or_else(|| anyhow!("sandbox-exec disappeared after startup"))?;
                let root = escape_sandbox_profile(self.guard.root());
                let policy = escape_sandbox_profile(&self.guard.root().join("everything.toml"));
                let profile = format!(
                    "(version 1)(deny default)(allow process*)(allow sysctl-read)(allow file-read*)(allow file-write* (subpath \"{root}\"))(deny file-write* (literal \"{policy}\"))(deny network*)"
                );
                let mut command = Command::new(sandbox);
                command
                    .args(["-p", &profile, "--", &request.program])
                    .args(&request.args)
                    .current_dir(working_directory);
                Ok(command)
            }
            SandboxBackend::GuardedProcess => {
                let mut command = Command::new(&request.program);
                command.args(&request.args).current_dir(working_directory);
                Ok(command)
            }
        }
    }
}

fn detect_sandbox_backend(enabled: bool) -> SandboxBackend {
    if !enabled {
        return SandboxBackend::GuardedProcess;
    }
    #[cfg(target_os = "linux")]
    if find_program("bwrap").is_some() {
        return SandboxBackend::Bubblewrap;
    }
    #[cfg(target_os = "macos")]
    if find_program("sandbox-exec").is_some() {
        return SandboxBackend::SandboxExec;
    }
    SandboxBackend::GuardedProcess
}

fn validate_program(
    program: &str,
    args: &[String],
    allowed: &BTreeSet<String>,
    trusted_workspace: bool,
    sandbox_backend: SandboxBackend,
) -> Result<()> {
    anyhow::ensure!(!program.trim().is_empty(), "program must not be empty");
    anyhow::ensure!(
        !program.contains('/') && !program.contains('\\'),
        "program paths are not allowed; use an executable name"
    );
    let normalized = normalize_program(program);
    anyhow::ensure!(
        !dangerous_programs().contains(normalized.as_str()),
        "program '{program}' is blocked by the non-bypassable dangerous-command policy"
    );
    let sandbox_supports_broad_access = match sandbox_backend {
        SandboxBackend::Bubblewrap => true,
        #[cfg(target_os = "macos")]
        SandboxBackend::SandboxExec => true,
        SandboxBackend::GuardedProcess => false,
    };
    anyhow::ensure!(
        allowed.contains(&normalized) || (trusted_workspace && sandbox_supports_broad_access),
        "program '{program}' is not allowlisted; broad execution requires TrustedWorkspace mode and an OS sandbox"
    );
    validate_arguments(&normalized, args)?;
    Ok(())
}

fn validate_arguments(program: &str, args: &[String]) -> Result<()> {
    validate_program_subcommand(program, args)?;
    let inline_eval = match program {
        "python" | "python3" | "node" | "ruby" | "perl" => &["-c", "-e", "--eval"][..],
        _ => &[][..],
    };
    for argument in args {
        anyhow::ensure!(
            !inline_eval.iter().any(|blocked| argument == blocked),
            "inline interpreter evaluation is blocked; execute a reviewed workspace file instead"
        );
        if looks_like_path(argument) {
            let path = Path::new(argument);
            anyhow::ensure!(
                !path.is_absolute(),
                "absolute command arguments are not allowed"
            );
            anyhow::ensure!(
                !path.components().any(|component| matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )),
                "command arguments may not escape the workspace"
            );
            anyhow::ensure!(
                !path.components().any(|component| matches!(component, Component::Normal(value) if value == OsStr::new(".everything"))),
                "command arguments may not directly target protected runtime metadata"
            );
            anyhow::ensure!(
                path != Path::new("everything.toml"),
                "command arguments may not directly target the runtime policy file"
            );
        }
    }
    Ok(())
}

fn validate_program_subcommand(program: &str, args: &[String]) -> Result<()> {
    let Some(subcommand) = primary_subcommand(program, args) else {
        return Ok(());
    };

    let blocked = match program {
        // Git is available for evidence and inspection. Repository mutations and
        // remote/network operations must go through typed Everything workflows.
        "git" => matches!(
            subcommand,
            "add"
                | "am"
                | "apply"
                | "branch"
                | "checkout"
                | "cherry-pick"
                | "clean"
                | "clone"
                | "commit"
                | "fetch"
                | "gc"
                | "merge"
                | "mv"
                | "pull"
                | "push"
                | "rebase"
                | "remote"
                | "reset"
                | "restore"
                | "revert"
                | "rm"
                | "stash"
                | "submodule"
                | "switch"
                | "tag"
                | "worktree"
        ),
        "cargo" => matches!(
            subcommand,
            "install"
                | "login"
                | "logout"
                | "owner"
                | "package"
                | "publish"
                | "search"
                | "uninstall"
                | "update"
                | "yank"
        ),
        "npm" | "pnpm" | "yarn" => matches!(
            subcommand,
            "add"
                | "audit"
                | "ci"
                | "create"
                | "deprecate"
                | "dist-tag"
                | "exec"
                | "init"
                | "install"
                | "link"
                | "login"
                | "logout"
                | "owner"
                | "pack"
                | "profile"
                | "publish"
                | "rebuild"
                | "remove"
                | "token"
                | "uninstall"
                | "unlink"
                | "unpublish"
                | "update"
        ),
        "dotnet" => matches!(subcommand, "add" | "new" | "nuget" | "remove" | "tool"),
        "go" => matches!(subcommand, "env" | "get" | "install" | "work"),
        _ => false,
    };
    anyhow::ensure!(
        !blocked,
        "'{program} {subcommand}' is blocked; use a typed Everything workflow or perform this irreversible/network-capable operation manually"
    );
    Ok(())
}

fn primary_subcommand<'a>(program: &str, args: &'a [String]) -> Option<&'a str> {
    let mut skip_next = false;
    for argument in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if program == "cargo" && argument.starts_with('+') {
            continue;
        }
        if argument.starts_with("--") {
            if !argument.contains('=') && option_takes_value(program, argument) {
                skip_next = true;
            }
            continue;
        }
        if argument.starts_with('-') {
            if option_takes_value(program, argument) && argument.len() == 2 {
                skip_next = true;
            }
            continue;
        }
        return Some(argument.as_str());
    }
    None
}

fn option_takes_value(program: &str, option: &str) -> bool {
    match program {
        "git" => matches!(
            option,
            "-C" | "-c"
                | "--git-dir"
                | "--work-tree"
                | "--namespace"
                | "--exec-path"
                | "--config-env"
                | "--super-prefix"
        ),
        "cargo" => matches!(
            option,
            "--manifest-path"
                | "--color"
                | "--config"
                | "--target-dir"
                | "--target"
                | "--jobs"
                | "-j"
        ),
        "npm" | "pnpm" | "yarn" => matches!(
            option,
            "--prefix" | "--cwd" | "--dir" | "--cache" | "--registry"
        ),
        "dotnet" => matches!(option, "--diagnostics"),
        "go" => matches!(option, "-C"),
        _ => false,
    }
}

fn looks_like_path(argument: &str) -> bool {
    argument.starts_with('/')
        || argument.starts_with("./")
        || argument.starts_with("../")
        || argument.contains('/')
        || argument.contains('\\')
        || (argument.len() >= 3
            && argument.as_bytes()[1] == b':'
            && matches!(argument.as_bytes()[2], b'/' | b'\\'))
}

fn dangerous_programs() -> BTreeSet<&'static str> {
    [
        "sh",
        "bash",
        "zsh",
        "fish",
        "dash",
        "ksh",
        "cmd",
        "cmd.exe",
        "powershell",
        "powershell.exe",
        "pwsh",
        "wscript",
        "cscript",
        "sudo",
        "su",
        "doas",
        "rm",
        "rmdir",
        "del",
        "erase",
        "dd",
        "mkfs",
        "fdisk",
        "diskpart",
        "format",
        "shutdown",
        "reboot",
        "halt",
        "poweroff",
        "mount",
        "umount",
        "chown",
        "chmod",
        "kill",
        "pkill",
        "killall",
        "taskkill",
        "reg",
        "reg.exe",
        "sc",
        "sc.exe",
        "launchctl",
        "systemctl",
        "curl",
        "wget",
        "ssh",
        "scp",
        "sftp",
        "nc",
        "ncat",
        "netcat",
        "socat",
    ]
    .into_iter()
    .collect()
}

fn normalize_program(program: &str) -> String {
    program.trim().to_ascii_lowercase()
}

fn find_program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = directory.join(format!("{name}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn copy_safe_environment(command: &mut Command) {
    for key in [
        "PATH",
        "TEMP",
        "TMP",
        "TMPDIR",
        "SYSTEMROOT",
        "COMSPEC",
        "LANG",
        "LC_ALL",
        "TERM",
        "PATHEXT",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn platform_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn escape_sandbox_profile(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[cfg(unix)]
fn termination_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn termination_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

fn read_limited(mut reader: impl Read, limit: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; 8192];
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

#[cfg(test)]
mod tests {
    use super::{
        SandboxBackend, dangerous_programs, primary_subcommand, validate_program,
        validate_program_subcommand,
    };
    use std::collections::BTreeSet;

    #[test]
    fn dangerous_programs_are_never_allowed() {
        let allowed = ["bash".to_owned()].into_iter().collect::<BTreeSet<_>>();
        assert!(validate_program("bash", &[], &allowed, true, SandboxBackend::Bubblewrap).is_err());
        assert!(dangerous_programs().contains("powershell"));
    }

    #[test]
    fn broad_program_requires_real_os_sandbox() {
        let allowed = BTreeSet::new();
        assert!(
            validate_program("go", &[], &allowed, true, SandboxBackend::GuardedProcess).is_err()
        );
        assert!(validate_program("go", &[], &allowed, true, SandboxBackend::Bubblewrap).is_ok());
    }

    #[test]
    fn network_and_irreversible_subcommands_are_blocked() {
        assert!(
            validate_program_subcommand("git", &["push".to_owned(), "origin".to_owned()]).is_err()
        );
        assert!(
            validate_program_subcommand(
                "git",
                &["-C".to_owned(), "repo".to_owned(), "push".to_owned()]
            )
            .is_err()
        );
        assert_eq!(
            primary_subcommand(
                "cargo",
                &[
                    "--manifest-path".to_owned(),
                    "Cargo.toml".to_owned(),
                    "test".to_owned()
                ]
            ),
            Some("test")
        );
        assert!(
            validate_program_subcommand("git", &["status".to_owned(), "--short".to_owned()])
                .is_ok()
        );
        assert!(validate_program_subcommand("cargo", &["publish".to_owned()]).is_err());
        assert!(validate_program_subcommand("cargo", &["test".to_owned()]).is_ok());
        assert!(validate_program_subcommand("npm", &["publish".to_owned()]).is_err());
        assert!(validate_program_subcommand("npm", &["run".to_owned(), "test".to_owned()]).is_ok());
    }

    #[test]
    fn inline_interpreter_code_is_blocked() {
        let allowed = ["python".to_owned()].into_iter().collect::<BTreeSet<_>>();
        assert!(
            validate_program(
                "python",
                &["-c".to_owned(), "print(1)".to_owned()],
                &allowed,
                false,
                SandboxBackend::GuardedProcess,
            )
            .is_err()
        );
    }
}
