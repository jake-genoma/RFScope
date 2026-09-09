# RFScope engineering guardrails

These instructions apply to the entire repository.

- Rust owns real-time radio, bounded ingestion, DSP, recording, playback, and server services. TypeScript/React owns workstation UX and render lifecycle. R owns analytical and data-science workflows and is never in the high-rate IQ hot path.
- Use the official `libhackrf` through isolated FFI. Keep device-specific behavior behind capability-driven device interfaces; the frontend must not assume HackRF forever or a single physical device.
- All radio-path queues are explicitly bounded. Never send high-rate IQ through JSON. Version binary streaming protocols. Visualization may drop stale frames; recording integrity takes priority over UI frame rate.
- Live and recorded IQ enter the same downstream DSP abstractions. Do not create a separate playback DSP stack.
- Document application APIs and protocol changes. Do not use hidden global mutable SDR state or hard-code capabilities that hardware can report.
- Do not use `unwrap()` or `expect()` in production hot paths unless impossibility is documented. Prefer structured, contextual errors.
- Bind networking to localhost by default. The initial product is RX-only; TX is out of scope until explicitly designed and reviewed.
- Run formatting, linting, tests, and relevant builds before committing. Keep milestone claims factual.
