use crate::catalog::build_module_catalog;
use anyhow::Result;
use everything_adapters::FileSystemAdapter;
use everything_domain::{BootstrapMetrics, ProjectDescriptor, WorkspaceSnapshot};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct BootstrapReport {
    pub project: ProjectDescriptor,
    pub snapshot: WorkspaceSnapshot,
    pub modules: Vec<everything_domain::ModuleDescriptor>,
    pub metrics: BootstrapMetrics,
}

pub struct WorkspaceBootstrapper {
    file_system: Arc<dyn FileSystemAdapter>,
    cache: Mutex<Option<CachedBootstrap>>,
}

struct CachedBootstrap {
    root_path: std::path::PathBuf,
    file_count: usize,
    modules: Vec<everything_domain::ModuleDescriptor>,
}

impl WorkspaceBootstrapper {
    pub fn new(file_system: Arc<dyn FileSystemAdapter>) -> Self {
        Self {
            file_system,
            cache: Mutex::new(None),
        }
    }

    pub fn bootstrap(&self, root: &Path) -> Result<BootstrapReport> {
        let snapshot_started = Instant::now();
        let snapshot = self.file_system.snapshot(root)?;
        let snapshot_millis = snapshot_started.elapsed().as_millis();

        let cached_modules = if snapshot.stats.cache_misses == 0 {
            self.cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .filter(|cached| {
                    cached.root_path == snapshot.root_path
                        && cached.file_count == snapshot.files.len()
                })
                .map(|cached| cached.modules.clone())
        } else {
            None
        };

        let catalog_started = Instant::now();
        let modules = cached_modules.unwrap_or_else(|| build_module_catalog(&snapshot));
        let catalog_millis = catalog_started.elapsed().as_millis();

        if snapshot.stats.cache_misses > 0
            || self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_none()
        {
            *self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(CachedBootstrap {
                root_path: snapshot.root_path.clone(),
                file_count: snapshot.files.len(),
                modules: modules.clone(),
            });
        }

        let project = ProjectDescriptor {
            root_path: snapshot.root_path.clone(),
            file_count: snapshot.files.len(),
            graph_node_count: 0,
            graph_edge_count: 0,
        };

        Ok(BootstrapReport {
            project,
            snapshot,
            modules,
            metrics: BootstrapMetrics {
                snapshot_millis,
                graph_millis: 0,
                catalog_millis,
            },
        })
    }
}
