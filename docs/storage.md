# Persistence boundaries

The server opens `RFSCOPE_DB` (default `rfscope.sqlite3`) and applies an idempotent, versioned SQLite migration. The schema stores sessions, recording indexes, workspaces, bookmarks, annotations, preferences, and derived observation payloads. Raw IQ remains in SigMF files. The storage API exposes indexed sessions and recordings at `/api/v1/sessions` and `/api/v1/storage/recordings`; recording start indexes the capture without touching the callback or writer path.

Retained analytical observations can be exported to Arrow-compatible Parquet with the versioned schema in `rf-engine::observations`. DuckDB can query these files directly; a dedicated DuckDB service/query API remains planned. SQLite is deliberately limited to transactional metadata and indexes.

The engine retains at most 4096 latest measurements in a bounded ring. `GET /api/v1/analysis/observations` reads that ring and `POST /api/v1/analysis/export` writes it to `RFSCOPE_OBSERVATIONS` (default `observations.parquet`).
