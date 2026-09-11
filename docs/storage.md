# Persistence boundaries

The server opens `RFSCOPE_DB` (default `rfscope.sqlite3`) and applies an idempotent, versioned SQLite migration. The schema stores sessions, recording indexes, workspaces, bookmarks, annotations, preferences, completed signal events, and derived observation payloads. Raw IQ remains in SigMF files. The storage API exposes indexed sessions and recordings at `/api/v1/sessions` and `/api/v1/storage/recordings`; recording start indexes the capture without touching the callback or writer path.

Retained analytical observations can be exported to Arrow-compatible Parquet with the versioned schema in `rf-engine::observations`. DuckDB can query these files directly; a dedicated DuckDB service/query API remains planned. SQLite is deliberately limited to transactional metadata and indexes.

The engine retains at most 4096 latest measurements in a bounded ring. `GET /api/v1/analysis/observations` reads that ring and `POST /api/v1/analysis/export` writes it to `RFSCOPE_OBSERVATIONS` (default `observations.parquet`).

Workspaces are stored transactionally with `GET/POST /api/v1/workspaces` and `DELETE /api/v1/workspaces/{id}`.

Bookmarks and annotations are stored transactionally with corresponding `GET/POST/DELETE` endpoints at `/api/v1/bookmarks` and `/api/v1/annotations`. Annotation payloads are validated JSON and are linked to a recording when one is supplied.

Completed SNR-threshold events move from the DSP loop into a bounded 128-event worker queue. The worker performs SQLite writes off the real-time path; `dropped_event_persistence` reports a saturated or unavailable event writer. `GET /api/v1/detections` combines the latest 4096 stored completed events with the bounded live event tracker.

When a DuckDB CLI is installed, `POST /api/v1/analysis/query` with `{ "path": "observations.parquet", "limit": 1000 }` queries the Parquet file through DuckDB's `read_parquet` table function. Set `RFSCOPE_DUCKDB_BIN` to select another executable. The endpoint is low-rate and isolated from the DSP thread; missing DuckDB is returned as a service-unavailable diagnostic.
