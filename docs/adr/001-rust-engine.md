# ADR: Rust real-time engine

Status: accepted

## Decision

Rust provides memory safety, predictable native performance, and a strong concurrency model. It owns ingestion, DSP, recording, playback, and service state.

## Consequences

The boundary is explicit and independently testable. Reversing it requires a documented migration rather than accidental coupling.
