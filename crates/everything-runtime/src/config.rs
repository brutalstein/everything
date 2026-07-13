use anyhow::Result;
use everything_domain::{ExecutionMode, RuntimeSettings, TaskRequest};
use std::path::{Path, PathBuf};

pub fn load_settings(workspace: &Path) -> Result<RuntimeSettings> {
    let workspace = workspace.canonicalize()?;
    let config_path = workspace.join("everything.toml");

    if config_path.exists() {
        let raw = std::fs::read_to_string(&config_path)?;
        let mut settings: RuntimeSettings = toml::from_str(&raw)?;
        settings.workspace_path = workspace;
        if settings.data_dir.is_relative() {
            settings.data_dir = settings.workspace_path.join(&settings.data_dir);
        }
        return Ok(settings);
    }

    Ok(RuntimeSettings::for_workspace(workspace))
}

pub fn build_task_request(
    settings: &RuntimeSettings,
    objective: String,
    mode: ExecutionMode,
) -> TaskRequest {
    TaskRequest {
        objective,
        mode,
        workspace_path: settings.workspace_path.clone(),
    }
}

pub fn ensure_data_dir(path: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    Ok(path.to_path_buf())
}
