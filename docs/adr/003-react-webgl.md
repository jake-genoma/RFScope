# ADR: React and WebGL frontend

Status: accepted

## Decision

React owns workstation controls and state; Canvas/WebGL2 renderer objects own high-rate pixels and typed arrays outside reconciliation.

## Consequences

The boundary is explicit and independently testable. Reversing it requires a documented migration rather than accidental coupling.
