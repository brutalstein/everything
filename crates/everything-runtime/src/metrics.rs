use crate::bootstrap::BootstrapReport;
use anyhow::Result;
use everything_domain::{BenchmarkRecord, RunJournal, RunSummary, RuntimeMetricsSnapshot};
use everything_state::StateStore;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const BENCHMARK_FILE: &str = "metrics/benchmarks.json";

pub fn summarize_journals(entries: Vec<(RunJournal, PathBuf)>) -> Vec<RunSummary> {
    entries
        .into_iter()
        .map(|(journal, path)| RunSummary {
            run_id: journal.run_id,
            objective: journal.objective,
            status: journal.status,
            generated_by: journal.generated_by,
            event_count: journal.events.len(),
            last_stage: journal.events.last().map(|event| event.stage.clone()),
            journal_path: path,
        })
        .collect()
}

pub fn load_benchmarks(base_dir: &Path) -> Result<Vec<BenchmarkRecord>> {
    let path = benchmark_store_path(base_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let payload = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&payload)?)
}

pub fn persist_benchmark(base_dir: &Path, record: &BenchmarkRecord) -> Result<()> {
    let path = benchmark_store_path(base_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut records = load_benchmarks(base_dir)?;
    records.push(record.clone());
    let temporary_path = path.with_extension("json.tmp");
    fs::write(&temporary_path, serde_json::to_string_pretty(&records)?)?;
    replace_file(&temporary_path, &path)?;
    Ok(())
}

pub fn metrics_snapshot(
    base_dir: &Path,
    state_store: &StateStore,
) -> Result<RuntimeMetricsSnapshot> {
    let runs = state_store.load_journals()?.entries;
    let benchmarks = load_benchmarks(base_dir)?;
    Ok(RuntimeMetricsSnapshot {
        data_dir: base_dir.to_path_buf(),
        runs_recorded: runs.len(),
        benchmarks_recorded: benchmarks.len(),
        latest_run_id: runs.first().map(|(journal, _)| journal.run_id.clone()),
        latest_benchmark_id: benchmarks.last().map(|record| record.benchmark_id.clone()),
    })
}

pub fn benchmark_store_path(base_dir: &Path) -> PathBuf {
    base_dir.join(BENCHMARK_FILE)
}

pub fn build_bootstrap_benchmark(
    workspace: &Path,
    iterations: usize,
    reports: &[BootstrapReport],
) -> BenchmarkRecord {
    let mut samples: Vec<u128> = reports
        .iter()
        .map(|report| {
            report.metrics.snapshot_millis
                + report.metrics.graph_millis
                + report.metrics.catalog_millis
        })
        .collect();
    samples.sort_unstable();

    let total_millis: u128 = samples.iter().copied().sum();
    let len = samples.len();
    let cache_hits = reports
        .iter()
        .map(|report| report.snapshot.stats.cache_hits)
        .sum();
    let cache_misses = reports
        .iter()
        .map(|report| report.snapshot.stats.cache_misses)
        .sum();
    let bytes_read = reports
        .iter()
        .map(|report| report.snapshot.stats.bytes_read)
        .sum();
    let created_at_epoch_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    BenchmarkRecord {
        benchmark_id: format!("bootstrap-{created_at_epoch_millis}"),
        benchmark_name: "bootstrap".to_owned(),
        workspace_path: workspace.to_path_buf(),
        iterations,
        mean_millis: total_millis as f64 / len as f64,
        min_millis: samples[0],
        max_millis: samples[len - 1],
        p50_millis: percentile(&samples, 0.50),
        p95_millis: percentile(&samples, 0.95),
        cache_hits,
        cache_misses,
        bytes_read,
        created_at_epoch_millis,
    }
}

fn percentile(samples: &[u128], quantile: f64) -> u128 {
    let last_index = samples.len().saturating_sub(1);
    let position = (last_index as f64 * quantile).round() as usize;
    samples[position.min(last_index)]
}

fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(source, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::percentile;

    #[test]
    fn percentile_uses_sorted_window() {
        let samples = vec![2, 4, 6, 8, 10];
        assert_eq!(percentile(&samples, 0.50), 6);
        assert_eq!(percentile(&samples, 0.95), 10);
    }
}
