# ADR: R analytics boundary

Status: accepted

## Decision

R is a first-class API and persisted-data client for statistics, reports, clustering, and RFScope Lab, but never participates in high-rate IQ processing.

## Consequences

The boundary is explicit and independently testable. Reversing it requires a documented migration rather than accidental coupling.
