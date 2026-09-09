# ADR: SigMF IQ recordings

Status: accepted

## Decision

SigMF is the primary portable IQ interchange. Unique session directories add settings history, events, annotations, audio, and analysis without corrupting interchange data.

## Consequences

The boundary is explicit and independently testable. Reversing it requires a documented migration rather than accidental coupling.
