# ADR: Tauri desktop shell

Status: accepted

## Decision

Tauri 2 packages the same Vite frontend used by remote browsers, avoiding a second desktop UI while retaining a Rust-native shell.

## Consequences

The boundary is explicit and independently testable. Reversing it requires a documented migration rather than accidental coupling.
