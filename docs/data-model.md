# Data model direction

Device descriptors, capabilities, mutable state, and diagnostics exist in `rf-types`. Future IDs are globally unique strings. VFOs belong to a capture window but are not physical tuners. RF event derived/classification fields remain optional and carry algorithm/version provenance.

SQLite indexes workspaces, preferences, recordings, annotations, and tags. IQ stays in unique SigMF session directories. High-volume observations use versioned Arrow-compatible Parquet schemas and DuckDB queries; raw IQ never enters a relational database.
