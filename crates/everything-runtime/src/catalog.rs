use everything_domain::{ModuleDescriptor, ModuleKind, WorkspaceSnapshot};
use regex::Regex;

pub fn build_module_catalog(snapshot: &WorkspaceSnapshot) -> Vec<ModuleDescriptor> {
    let package_regex = Regex::new(r#"(?m)^\s*name\s*=\s*"([^"]+)""#).expect("valid package regex");
    let mut modules = Vec::new();

    for file in snapshot.files.iter().filter(|file| {
        file.relative_path
            .file_name()
            .is_some_and(|name| name == "Cargo.toml")
    }) {
        let Some(captures) = package_regex.captures(&file.content) else {
            continue;
        };

        let name = captures
            .get(1)
            .map(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        let kind = if name.contains("-cli") {
            ModuleKind::EntryPoint
        } else if name.contains("adapter") {
            ModuleKind::Adapter
        } else if file
            .relative_path
            .components()
            .any(|value| value.as_os_str() == "tests")
        {
            ModuleKind::Tests
        } else {
            ModuleKind::Core
        };

        let responsibility = match name.as_str() {
            "everything-domain" => "Stable domain types and runtime contracts.".to_owned(),
            "everything-graph" => "Project graph extraction, indexing, and traversal.".to_owned(),
            "everything-adapters" => "Port traits and default local implementations.".to_owned(),
            "everything-runtime" => "Deterministic orchestration and planning flow.".to_owned(),
            "everything-cli" => "Operator-facing command line entry point.".to_owned(),
            _ => "Extensible module boundary.".to_owned(),
        };

        modules.push(ModuleDescriptor {
            name,
            kind,
            responsibility,
        });
    }

    modules.sort_by(|left, right| left.name.cmp(&right.name));
    modules
}
