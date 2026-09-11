//! Optional low-rate DuckDB CLI boundary for Parquet analytics.
use serde_json::Value;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DuckdbError {
    #[error("DuckDB executable is unavailable: {0}")]
    Unavailable(#[from] std::io::Error),
    #[error("DuckDB query failed: {0}")]
    Query(String),
    #[error("DuckDB returned invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Execute a low-rate analytical query through the locally installed DuckDB CLI.
/// The SQL is passed as an argument, never through a shell.
pub fn query(sql: &str) -> Result<Value, DuckdbError> {
    let executable = std::env::var_os("RFSCOPE_DUCKDB_BIN").unwrap_or_else(|| "duckdb".into());
    let output = std::process::Command::new(executable)
        .args(["-json", "-c", sql])
        .output()?;
    if !output.status.success() {
        return Err(DuckdbError::Query(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Build a parameter-free query for an exported Parquet file with a bounded result set.
pub fn parquet_query(path: impl AsRef<Path>, limit: usize) -> String {
    let escaped = path.as_ref().display().to_string().replace('\'', "''");
    format!(
        "SELECT * FROM read_parquet('{escaped}') LIMIT {}",
        limit.min(100_000)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parquet_query_escapes_paths_and_bounds_limit() {
        let query = parquet_query("/tmp/a'b.parquet", usize::MAX);
        assert!(query.contains("a''b.parquet"));
        assert!(query.ends_with("LIMIT 100000"));
    }
}
