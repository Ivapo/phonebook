pub mod migrations;

use anyhow::Context;
use rusqlite::Connection;

pub fn init_db(path: &str) -> anyhow::Result<Connection> {
    let conn = Connection::open(path).context("failed to open database")?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .context("failed to set database pragmas")?;

    migrations::run_migrations(&conn)?;

    Ok(conn)
}
