# Architecture

Rust owns ingestion, bounded queues, DSP, recording/playback, device state, and network services. React owns controls and presentation; renderer classes own pixel lifecycles outside React. R owns reproducible historical/statistical workflows.

`IqSource` is the common source seam. `MockSource`, hardware `BufferedSource`, and `SigmfSource` feed complex blocks into `Engine`; playback uses the same VFO, demodulation, audio, and spectrum path. The engine publishes latest-useful spectrum frames through a bounded Tokio broadcast channel (capacity four). Slow visualization clients lag/drop rather than creating unbounded history. Recording branches from raw IQ before display DSP with a separately bounded, integrity-prioritized writer path.

CPU DSP remains outside HTTP handlers. Tokio operates control/networking; a separate thread with its own lightweight runtime drains IQ and performs spectrum DSP. State is explicit and owned by `Engine`, not hidden globally.

The browser uses JSON only for low-rate state. It receives little-endian binary PSD frames, draws spectrum with Canvas, and maintains waterfall history in a WebGL2 ring texture. Tauri loads the identical frontend. Server binding is localhost-only by default.

Future physical devices are collections of capability-driven instances. A VFO is a simultaneous software channel within one capture; HackRF sweep ranges are time-multiplexed; only distinct devices provide independently tuned simultaneous RF windows.

Storage will separate SQLite application metadata, SigMF IQ datasets, and Parquet analytical observations queried with DuckDB. R/rfscopeR and future **RFScope Lab** (optionally Shiny) consume documented APIs and those files for historical exploration, comparison, reports, clustering, and experiments—never the IQ hot path.

Plugins will begin as versioned data/message contracts. R/Python and risky decoders should be out-of-process; native Rust/C DSP extensions require a separately reviewed ABI and isolation strategy.

The optional `rf-device/hackrf` backend owns the native receiver on a dedicated
control thread with an eight-request queue. The official RX callback copies raw
signed interleaved bytes into a preallocated 16 × 262,144-byte pool using a
nonblocking lock attempt. Contention/exhaustion is counted, never hidden.
`BufferedSource` moves a block out of the pool, normalizes it outside the callback,
and returns it to the pool. `IqSource::read` returns the valid complex sample count;
`NoData` is transient. No native pointer crosses into the engine.

An explicit one-slot source mailbox connects the owner to the dedicated DSP thread.
Every RX restart creates a fresh source with immutable capture settings. Current
hardware configuration conservatively stops RX, configures, and restarts if it was
running, preventing old buffered IQ from being labelled with new settings.
The engine continuously drains hardware, caches FFT plans/buffers, and limits
spectrum production to at most 25 FPS. Stop, close and native stream health are
explicit; shutdown joins DSP and closes the native owner. Mock retains its demo
pacing. Live hardware and mock share the spectrum analyzer and encoder.

Recording still needs an integrity-prioritized raw branch before conversion.
The current source pool deliberately reports overload drops and does not claim
recording integrity. See [RX validation](rx-validation.md).

VFO output then passes through the fixed-rate audio stage and a bounded broadcast
PCM bus. Audio transport is downstream of IQ and cannot block ingestion.

Raw IQ can also tee to the bounded SigMF writer before conversion. Its dedicated
pool/queue is independent of display/audio; overload is explicit and recording
completion fails rather than silently claiming a complete capture.

The same complex blocks feed `VfoProcessor` before display frame selection.
Receiver configurations are copied from a short-held registry lock; channel DSP
state and output buffers remain local to the DSP thread. CPU admission uses a
work budget rather than a fixed number of receivers. See [virtual receivers](vfo.md).
