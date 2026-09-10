# Persistence boundaries

The server opens `RFSCOPE_DB` (default `rfscope.sqlite3`) and applies an idempotent, versioned SQLite migration. The schema stores sessions, recording indexes, workspaces, bookmarks, annotations, preferences, and derived observation payloads. Raw IQ remains in SigMF files. The storage API exposes indexed sessions and recordings at `/api/v1/sessions` and `/api/v1/storage/recordings`; recording start indexes the capture without touching the callback or writer path.

Parquet and DuckDB remain planned for high volume analytical observations. SQLite is deliberately limited to transactional metadata and indexes.
