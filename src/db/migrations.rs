use anyhow::Context;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

pub fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .context("failed to create migrations table")?;

    let migrations_dir = Path::new("migrations");
    if !migrations_dir.exists() {
        tracing::warn!("migrations directory not found, skipping");
        return Ok(());
    }

    let mut entries: Vec<_> = fs::read_dir(migrations_dir)
        .context("failed to read migrations directory")?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "sql")
                .unwrap_or(false)
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = ?1",
                [&name],
                |row| row.get(0),
            )
            .context("failed to check migration status")?;

        if already_applied {
            continue;
        }

        let sql = fs::read_to_string(entry.path())
            .with_context(|| format!("failed to read migration file: {name}"))?;

        conn.execute_batch(&sql)
            .with_context(|| format!("failed to apply migration: {name}"))?;

        conn.execute("INSERT INTO _migrations (name) VALUES (?1)", [&name])
            .with_context(|| format!("failed to record migration: {name}"))?;

        tracing::info!("applied migration: {name}");
    }

    Ok(())
}
