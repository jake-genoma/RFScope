//! Versioned transactional metadata storage. Raw IQ remains in SigMF files.
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{path::Path, sync::Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("storage lock poisoned")]
    Poisoned,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct StoredRecording {
    pub id: String,
    pub directory: String,
    pub sample_rate_hz: u32,
    pub center_frequency_hz: u64,
    pub created_at_unix_ns: u64,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct StoredSession {
    pub id: String,
    pub source: String,
    pub center_frequency_hz: u64,
    pub sample_rate_hz: u32,
    pub started_at_unix_ns: u64,
}
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
pub struct StoredWorkspace {
    pub id: String,
    pub name: String,
    pub payload_json: String,
}

pub struct Storage {
    connection: Mutex<Connection>,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let connection = Connection::open(path)?;
        let storage = Self {
            connection: Mutex::new(connection),
        };
        storage.migrate()?;
        Ok(storage)
    }
    pub fn in_memory() -> Result<Self, StorageError> {
        let storage = Self {
            connection: Mutex::new(Connection::open_in_memory()?),
        };
        storage.migrate()?;
        Ok(storage)
    }
    pub fn migrate(&self) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, source TEXT NOT NULL, center_frequency_hz INTEGER NOT NULL, sample_rate_hz INTEGER NOT NULL, started_at_unix_ns INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS recordings (id TEXT PRIMARY KEY, session_id TEXT REFERENCES sessions(id), directory TEXT NOT NULL, sample_rate_hz INTEGER NOT NULL, center_frequency_hz INTEGER NOT NULL, created_at_unix_ns INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS workspaces (id TEXT PRIMARY KEY, name TEXT NOT NULL, payload_json TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS bookmarks (id INTEGER PRIMARY KEY, recording_id TEXT REFERENCES recordings(id), sample_index INTEGER NOT NULL, label TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS annotations (id INTEGER PRIMARY KEY, recording_id TEXT REFERENCES recordings(id), start_sample INTEGER NOT NULL, end_sample INTEGER NOT NULL, payload_json TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS preferences (key TEXT PRIMARY KEY, value_json TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS observations (id INTEGER PRIMARY KEY, session_id TEXT REFERENCES sessions(id), observed_at_unix_ns INTEGER NOT NULL, payload_json TEXT NOT NULL);",
        )?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 1 {
            connection.execute(
                "INSERT OR IGNORE INTO schema_migrations(version) VALUES (1)",
                [],
            )?;
            connection.pragma_update(None, "user_version", 1)?;
        }
        Ok(())
    }
    pub fn record_session(
        &self,
        id: &str,
        source: &str,
        center: u64,
        rate: u32,
        started: u64,
    ) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute("INSERT OR REPLACE INTO sessions(id,source,center_frequency_hz,sample_rate_hz,started_at_unix_ns) VALUES (?1,?2,?3,?4,?5)", params![id, source, center, rate, started])?;
        Ok(())
    }
    pub fn index_recording(
        &self,
        recording: &StoredRecording,
        session_id: Option<&str>,
    ) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute("INSERT OR REPLACE INTO recordings(id,session_id,directory,sample_rate_hz,center_frequency_hz,created_at_unix_ns) VALUES (?1,?2,?3,?4,?5,?6)", params![recording.id, session_id, recording.directory, recording.sample_rate_hz, recording.center_frequency_hz, recording.created_at_unix_ns])?;
        Ok(())
    }
    pub fn recordings(&self) -> Result<Vec<StoredRecording>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement = connection.prepare("SELECT id,directory,sample_rate_hz,center_frequency_hz,created_at_unix_ns FROM recordings ORDER BY created_at_unix_ns")?;
        let rows = statement.query_map([], |row| {
            Ok(StoredRecording {
                id: row.get(0)?,
                directory: row.get(1)?,
                sample_rate_hz: row.get(2)?,
                center_frequency_hz: row.get(3)?,
                created_at_unix_ns: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }
    pub fn sessions(&self) -> Result<Vec<StoredSession>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement = connection.prepare("SELECT id,source,center_frequency_hz,sample_rate_hz,started_at_unix_ns FROM sessions ORDER BY started_at_unix_ns")?;
        let rows = statement.query_map([], |row| {
            Ok(StoredSession {
                id: row.get(0)?,
                source: row.get(1)?,
                center_frequency_hz: row.get(2)?,
                sample_rate_hz: row.get(3)?,
                started_at_unix_ns: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }
    pub fn workspaces(&self) -> Result<Vec<StoredWorkspace>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement =
            connection.prepare("SELECT id,name,payload_json FROM workspaces ORDER BY name")?;
        let rows = statement.query_map([], |row| {
            Ok(StoredWorkspace {
                id: row.get(0)?,
                name: row.get(1)?,
                payload_json: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }
    pub fn upsert_workspace(&self, workspace: &StoredWorkspace) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute(
            "INSERT OR REPLACE INTO workspaces(id,name,payload_json) VALUES (?1,?2,?3)",
            params![workspace.id, workspace.name, workspace.payload_json],
        )?;
        Ok(())
    }
    pub fn remove_workspace(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        Ok(connection.execute("DELETE FROM workspaces WHERE id = ?1", params![id])? != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_is_idempotent_and_indexes_recordings() {
        let storage = Storage::in_memory().unwrap();
        storage.migrate().unwrap();
        storage
            .record_session("s1", "mock", 100_000_000, 2_000_000, 1)
            .unwrap();
        storage
            .index_recording(
                &StoredRecording {
                    id: "r1".into(),
                    directory: "/tmp/r1".into(),
                    sample_rate_hz: 2_000_000,
                    center_frequency_hz: 100_000_000,
                    created_at_unix_ns: 2,
                },
                Some("s1"),
            )
            .unwrap();
        assert_eq!(storage.recordings().unwrap().len(), 1);
        storage
            .upsert_workspace(&StoredWorkspace {
                id: "w1".into(),
                name: "Live".into(),
                payload_json: "{}".into(),
            })
            .unwrap();
        assert_eq!(storage.workspaces().unwrap()[0].id, "w1");
        assert!(storage.remove_workspace("w1").unwrap());
    }
}
