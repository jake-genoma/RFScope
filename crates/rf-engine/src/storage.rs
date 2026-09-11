//! Versioned transactional metadata storage. Raw IQ remains in SigMF files.
use rf_types::SignalEvent;
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
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
pub struct StoredBookmark {
    pub id: i64,
    pub recording_id: Option<String>,
    pub sample_index: u64,
    pub label: String,
}
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
pub struct StoredAnnotation {
    pub id: i64,
    pub recording_id: Option<String>,
    pub start_sample: u64,
    pub end_sample: u64,
    pub payload_json: String,
}
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
pub struct StoredSignalEvent {
    pub id: String,
    pub start_frequency_hz: u64,
    pub end_frequency_hz: u64,
    pub start_time_unix_ns: u64,
    pub end_time_unix_ns: u64,
    pub peak_dbfs: f32,
    pub snr_db: f32,
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
             CREATE TABLE IF NOT EXISTS observations (id INTEGER PRIMARY KEY, session_id TEXT REFERENCES sessions(id), observed_at_unix_ns INTEGER NOT NULL, payload_json TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS signal_events (id TEXT PRIMARY KEY, start_frequency_hz INTEGER NOT NULL, end_frequency_hz INTEGER NOT NULL, start_time_unix_ns INTEGER NOT NULL, end_time_unix_ns INTEGER NOT NULL, peak_dbfs REAL NOT NULL, snr_db REAL NOT NULL);",
        )?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 1 {
            connection.execute(
                "INSERT OR IGNORE INTO schema_migrations(version) VALUES (1)",
                [],
            )?;
            connection.pragma_update(None, "user_version", 1)?;
        }
        if version < 2 {
            connection.execute(
                "INSERT OR IGNORE INTO schema_migrations(version) VALUES (2)",
                [],
            )?;
            connection.pragma_update(None, "user_version", 2)?;
        }
        Ok(())
    }
    pub fn record_signal_event(&self, event: &SignalEvent) -> Result<(), StorageError> {
        let Some(end_time_unix_ns) = event.end_time_unix_ns else {
            return Ok(());
        };
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute(
            "INSERT OR REPLACE INTO signal_events(id,start_frequency_hz,end_frequency_hz,start_time_unix_ns,end_time_unix_ns,peak_dbfs,snr_db) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![event.id, event.start_frequency_hz, event.end_frequency_hz, event.start_time_unix_ns, end_time_unix_ns, event.peak_dbfs, event.snr_db],
        )?;
        Ok(())
    }
    pub fn signal_events(&self, limit: usize) -> Result<Vec<StoredSignalEvent>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement = connection.prepare("SELECT id,start_frequency_hz,end_frequency_hz,start_time_unix_ns,end_time_unix_ns,peak_dbfs,snr_db FROM signal_events ORDER BY end_time_unix_ns DESC LIMIT ?1")?;
        let rows = statement.query_map(params![limit.min(4096) as i64], |row| {
            Ok(StoredSignalEvent {
                id: row.get(0)?,
                start_frequency_hz: row.get(1)?,
                end_frequency_hz: row.get(2)?,
                start_time_unix_ns: row.get(3)?,
                end_time_unix_ns: row.get(4)?,
                peak_dbfs: row.get(5)?,
                snr_db: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
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
    pub fn bookmarks(&self) -> Result<Vec<StoredBookmark>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement = connection.prepare(
            "SELECT id,recording_id,sample_index,label FROM bookmarks ORDER BY sample_index",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredBookmark {
                id: row.get(0)?,
                recording_id: row.get(1)?,
                sample_index: row.get(2)?,
                label: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }
    pub fn create_bookmark(
        &self,
        bookmark: &StoredBookmark,
    ) -> Result<StoredBookmark, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute(
            "INSERT INTO bookmarks(recording_id,sample_index,label) VALUES (?1,?2,?3)",
            params![bookmark.recording_id, bookmark.sample_index, bookmark.label],
        )?;
        Ok(StoredBookmark {
            id: connection.last_insert_rowid(),
            ..bookmark.clone()
        })
    }
    pub fn remove_bookmark(&self, id: i64) -> Result<bool, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        Ok(connection.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])? != 0)
    }
    pub fn annotations(&self) -> Result<Vec<StoredAnnotation>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement = connection.prepare("SELECT id,recording_id,start_sample,end_sample,payload_json FROM annotations ORDER BY start_sample")?;
        let rows = statement.query_map([], |row| {
            Ok(StoredAnnotation {
                id: row.get(0)?,
                recording_id: row.get(1)?,
                start_sample: row.get(2)?,
                end_sample: row.get(3)?,
                payload_json: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }
    pub fn create_annotation(
        &self,
        annotation: &StoredAnnotation,
    ) -> Result<StoredAnnotation, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute("INSERT INTO annotations(recording_id,start_sample,end_sample,payload_json) VALUES (?1,?2,?3,?4)", params![annotation.recording_id, annotation.start_sample, annotation.end_sample, annotation.payload_json])?;
        Ok(StoredAnnotation {
            id: connection.last_insert_rowid(),
            ..annotation.clone()
        })
    }
    pub fn remove_annotation(&self, id: i64) -> Result<bool, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        Ok(connection.execute("DELETE FROM annotations WHERE id = ?1", params![id])? != 0)
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
        let bookmark = storage
            .create_bookmark(&StoredBookmark {
                id: 0,
                recording_id: Some("r1".into()),
                sample_index: 42,
                label: "interesting".into(),
            })
            .unwrap();
        assert_eq!(storage.bookmarks().unwrap()[0], bookmark);
        assert!(storage.remove_bookmark(bookmark.id).unwrap());
        let annotation = storage
            .create_annotation(&StoredAnnotation {
                id: 0,
                recording_id: Some("r1".into()),
                start_sample: 10,
                end_sample: 20,
                payload_json: "{\"label\":\"burst\"}".into(),
            })
            .unwrap();
        assert_eq!(storage.annotations().unwrap()[0], annotation);
        assert!(storage.remove_annotation(annotation.id).unwrap());
        storage
            .record_signal_event(&SignalEvent {
                id: "event-1".into(),
                start_frequency_hz: 100,
                end_frequency_hz: 101,
                start_time_unix_ns: 10,
                end_time_unix_ns: Some(20),
                peak_dbfs: -10.0,
                snr_db: 15.0,
            })
            .unwrap();
        assert_eq!(storage.signal_events(10).unwrap()[0].id, "event-1");
    }
}
