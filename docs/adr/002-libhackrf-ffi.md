# ADR: Official libhackrf FFI

Status: accepted

## Decision

Use official libhackrf through a minimal feature-gated FFI module in `rf-device` and safe wrapper. This preserves vendor support while containing unsafe code.

## Consequences

The boundary is explicit and independently testable. Reversing it requires a documented migration rather than accidental coupling.

## Control milestone implementation

Choose minimal manual bindings to the installed official header, avoiding a
bindgen/libclang dependency for this small stable C surface. `build.rs` asks
pkg-config for native link flags only when the `hackrf` feature is enabled.
The private backend owns all unsafe operations. A dedicated thread plus safe
reference-counted context/RAII handles enforces lifecycle without unsafe Send or
Sync. A single process lifecycle atomic guards libhackrf initialization, not SDR
state. The bounded control queue and generic receiver-control trait are separate
from the unchanged IQ source interface until RX streaming is implemented.

The planned separate FFI crate is unnecessary for this small surface: module
privacy plus Cargo feature isolation provides the boundary without adding a
workspace crate that default hardware-independent builds would need to link.
Capability discovery uses queried board identity with documented backend profiles
where libhackrf exposes no range query; the API labels that provenance explicitly.
Native configuration failures close ownership because the setter sequence cannot
be rolled back reliably. No TX symbols are bound.
