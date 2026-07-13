use crate::ports::{CommandExecutor, CommandOutput, CommandRequest};
use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Default)]
pub struct LocalCommandExecutor;

impl CommandExecutor for LocalCommandExecutor {
    fn name(&self) -> &str {
        "local-command"
    }

    fn execute(&self, request: CommandRequest) -> Result<CommandOutput> {
        let output = Command::new(&request.program)
            .args(&request.args)
            .current_dir(&request.working_directory)
            .output()
            .with_context(|| format!("failed to execute {}", request.program))?;

        Ok(CommandOutput {
            status_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
