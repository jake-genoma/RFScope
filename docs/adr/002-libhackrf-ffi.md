# ADR: Official libhackrf FFI

Status: accepted

## Decision

Use official libhackrf through a minimal feature-gated FFI crate and safe wrapper. This preserves vendor support while containing unsafe code.

## Consequences

The boundary is explicit and independently testable. Reversing it requires a documented migration rather than accidental coupling.
