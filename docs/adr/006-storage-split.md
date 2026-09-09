# ADR: Storage by workload

Status: accepted

## Decision

SQLite serves transactional metadata, Parquet serves high-volume observations, and DuckDB queries analytical data. Raw IQ remains in SigMF files.

## Consequences

The boundary is explicit and independently testable. Reversing it requires a documented migration rather than accidental coupling.
