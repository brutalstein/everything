use anyhow::{Context, Result, anyhow};
use std::io::{Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PlatformSecretVault {
    namespace: String,
}

impl PlatformSecretVault {
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        #[cfg(target_os = "macos")]
        {
            "macos-keychain"
        }
        #[cfg(target_os = "linux")]
        {
            "libsecret"
        }
        #[cfg(windows)]
        {
            "windows-credential-manager"
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
        {
            "unsupported"
        }
    }

    pub fn available(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            command_exists("security")
        }
        #[cfg(target_os = "linux")]
        {
            command_exists("secret-tool")
        }
        #[cfg(windows)]
        {
            command_exists("powershell") || command_exists("pwsh")
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
        {
            false
        }
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        validate_key(key)?;
        anyhow::ensure!(
            self.available(),
            "OS secret vault '{}' is unavailable",
            self.backend_name()
        );
        #[cfg(target_os = "macos")]
        {
            let mut command = Command::new("security");
            command.args([
                "add-generic-password",
                "-U",
                "-a",
                key,
                "-s",
                &self.namespace,
                "-w",
                value,
            ]);
            let output = run_bounded(command, None, "write macOS Keychain secret")?;
            anyhow::ensure!(
                output.status.success(),
                "macOS Keychain rejected the secret: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            let mut command = Command::new("secret-tool");
            command.args([
                "store",
                "--label",
                "Everything connector secret",
                "service",
                &self.namespace,
                "account",
                key,
            ]);
            let output = run_bounded(command, Some(value.as_bytes()), "write libsecret secret")?;
            anyhow::ensure!(
                output.status.success(),
                "libsecret rejected the secret: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Ok(());
        }
        #[cfg(windows)]
        {
            return run_windows_vault("write", &self.namespace, key, Some(value)).map(|_| ());
        }
        #[allow(unreachable_code)]
        Err(anyhow!("secret vault is unsupported on this platform"))
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        validate_key(key)?;
        if let Some(value) = env_secret(key) {
            return Ok(Some(value));
        }
        anyhow::ensure!(
            self.available(),
            "OS secret vault '{}' is unavailable",
            self.backend_name()
        );
        #[cfg(target_os = "macos")]
        {
            let mut command = Command::new("security");
            command.args([
                "find-generic-password",
                "-a",
                key,
                "-s",
                &self.namespace,
                "-w",
            ]);
            let output = run_bounded(command, None, "read macOS Keychain secret")?;
            if !output.status.success() {
                return Ok(None);
            }
            return Ok(Some(
                String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_owned(),
            ));
        }
        #[cfg(target_os = "linux")]
        {
            let mut command = Command::new("secret-tool");
            command.args(["lookup", "service", &self.namespace, "account", key]);
            let output = run_bounded(command, None, "read libsecret secret")?;
            if !output.status.success() || output.stdout.is_empty() {
                return Ok(None);
            }
            return Ok(Some(
                String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_owned(),
            ));
        }
        #[cfg(windows)]
        {
            let value = run_windows_vault("read", &self.namespace, key, None)?;
            return if value.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(value.trim_end().to_owned()))
            };
        }
        #[allow(unreachable_code)]
        Err(anyhow!("secret vault is unsupported on this platform"))
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;
        if !self.available() {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        {
            let mut command = Command::new("security");
            command.args(["delete-generic-password", "-a", key, "-s", &self.namespace]);
            let _ = run_bounded(command, None, "delete macOS Keychain secret");
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            let mut command = Command::new("secret-tool");
            command.args(["clear", "service", &self.namespace, "account", key]);
            let _ = run_bounded(command, None, "delete libsecret secret");
            return Ok(());
        }
        #[cfg(windows)]
        {
            let _ = run_windows_vault("delete", &self.namespace, key, None);
            return Ok(());
        }
        #[allow(unreachable_code)]
        Ok(())
    }
}

#[derive(Debug)]
struct BoundedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_bounded(
    mut command: Command,
    input: Option<&[u8]>,
    context: &'static str,
) -> Result<BoundedOutput> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().with_context(|| context.to_owned())?;
    if let Some(input) = input {
        child
            .stdin
            .as_mut()
            .context("open secret-vault helper stdin")?
            .write_all(input)?;
    }
    drop(child.stdin.take());
    let stdout = child
        .stdout
        .take()
        .context("capture secret-vault helper stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("capture secret-vault helper stderr")?;
    let stdout_thread = std::thread::spawn(move || read_limited(stdout, 256 * 1024));
    let stderr_thread = std::thread::spawn(move || read_limited(stderr, 256 * 1024));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            anyhow::bail!("{context} timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_limited(mut reader: impl Read, limit: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(limit.min(16 * 1024));
    let mut buffer = [0_u8; 4096];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let remaining = limit.saturating_sub(output.len());
        if remaining > 0 {
            output.extend_from_slice(&buffer[..count.min(remaining)]);
        }
    }
    output
}

fn env_secret(key: &str) -> Option<String> {
    let normalized = key
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    std::env::var(format!("EVERYTHING_SECRET_{normalized}")).ok()
}

fn validate_key(key: &str) -> Result<()> {
    anyhow::ensure!(!key.trim().is_empty(), "secret key must not be empty");
    anyhow::ensure!(
        key.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')),
        "secret key contains unsupported characters"
    );
    Ok(())
}

fn command_exists(program: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(program);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            return directory.join(format!("{program}.exe")).is_file();
        }
        #[cfg(not(windows))]
        false
    })
}

#[cfg(windows)]
fn run_windows_vault(
    action: &str,
    namespace: &str,
    key: &str,
    value: Option<&str>,
) -> Result<String> {
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class EverythingCredentialNative {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  public struct CREDENTIAL {
    public UInt32 Flags; public UInt32 Type; public string TargetName; public string Comment;
    public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten; public UInt32 CredentialBlobSize;
    public IntPtr CredentialBlob; public UInt32 Persist; public UInt32 AttributeCount; public IntPtr Attributes;
    public string TargetAlias; public string UserName;
  }
  [DllImport("Advapi32.dll", EntryPoint="CredWriteW", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern bool CredWrite(ref CREDENTIAL credential, UInt32 flags);
  [DllImport("Advapi32.dll", EntryPoint="CredReadW", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern bool CredRead(string target, UInt32 type, UInt32 flags, out IntPtr credential);
  [DllImport("Advapi32.dll", EntryPoint="CredDeleteW", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern bool CredDelete(string target, UInt32 type, UInt32 flags);
  [DllImport("Advapi32.dll", SetLastError=true)] public static extern void CredFree(IntPtr buffer);
}
'@
$target = "$env:EVERYTHING_VAULT_NAMESPACE/$env:EVERYTHING_VAULT_KEY"
switch ($env:EVERYTHING_VAULT_ACTION) {
  'write' {
    $bytes = [Text.Encoding]::Unicode.GetBytes($env:EVERYTHING_VAULT_VALUE)
    $ptr = [Runtime.InteropServices.Marshal]::AllocCoTaskMem($bytes.Length)
    try {
      [Runtime.InteropServices.Marshal]::Copy($bytes, 0, $ptr, $bytes.Length)
      $cred = New-Object EverythingCredentialNative+CREDENTIAL
      $cred.Type = 1; $cred.TargetName = $target; $cred.CredentialBlobSize = $bytes.Length
      $cred.CredentialBlob = $ptr; $cred.Persist = 2; $cred.UserName = 'Everything'
      if (-not [EverythingCredentialNative]::CredWrite([ref]$cred, 0)) { throw "CredWrite failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    } finally { [Runtime.InteropServices.Marshal]::FreeCoTaskMem($ptr) }
  }
  'read' {
    $ptr = [IntPtr]::Zero
    if (-not [EverythingCredentialNative]::CredRead($target, 1, 0, [ref]$ptr)) { exit 3 }
    try {
      $cred = [Runtime.InteropServices.Marshal]::PtrToStructure($ptr, [type][EverythingCredentialNative+CREDENTIAL])
      if ($cred.CredentialBlobSize -gt 0) {
        $bytes = New-Object byte[] $cred.CredentialBlobSize
        [Runtime.InteropServices.Marshal]::Copy($cred.CredentialBlob, $bytes, 0, $bytes.Length)
        [Console]::Out.Write([Text.Encoding]::Unicode.GetString($bytes))
      }
    } finally { [EverythingCredentialNative]::CredFree($ptr) }
  }
  'delete' { [void][EverythingCredentialNative]::CredDelete($target, 1, 0) }
}
"#;
    let program = if command_exists("pwsh") {
        "pwsh"
    } else {
        "powershell"
    };
    let mut command = Command::new(program);
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", "-"])
        .env("EVERYTHING_VAULT_ACTION", action)
        .env("EVERYTHING_VAULT_NAMESPACE", namespace)
        .env("EVERYTHING_VAULT_KEY", key)
        .env("EVERYTHING_VAULT_VALUE", value.unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_bounded(
        command,
        Some(SCRIPT.as_bytes()),
        "run PowerShell credential helper",
    )?;
    if action == "read" && output.status.code() == Some(3) {
        return Ok(String::new());
    }
    anyhow::ensure!(
        output.status.success(),
        "Windows Credential Manager operation failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
