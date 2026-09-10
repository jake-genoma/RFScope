# Architecture

Rust owns ingestion, bounded queues, DSP, recording/playback, device state, and network services. React owns controls and presentation; renderer classes own pixel lifecycles outside React. R owns reproducible historical/statistical workflows.

`IqSource` is the common source seam. Today `MockSource` feeds complex blocks into `Engine`; a live HackRF source and SigMF file source will feed that same seam. The engine publishes latest-useful spectrum frames through a bounded Tokio broadcast channel (capacity four). Slow visualization clients lag/drop rather than creating unbounded history. Recording will branch from raw IQ before display DSP with a separately bounded, integrity-prioritized writer path.

CPU DSP remains outside HTTP handlers. Tokio operates control/networking and schedules the demo; production high-rate DSP will use dedicated workers. State is explicit and owned by `Engine`, not hidden globally.

The browser uses JSON only for low-rate state. It receives little-endian binary PSD frames, draws spectrum with Canvas, and maintains waterfall history in a WebGL2 ring texture. Tauri loads the identical frontend. Server binding is localhost-only by default.

Future physical devices are collections of capability-driven instances. A VFO is a simultaneous software channel within one capture; HackRF sweep ranges are time-multiplexed; only distinct devices provide independently tuned simultaneous RF windows.

Storage will separate SQLite application metadata, SigMF IQ datasets, and Parquet analytical observations queried with DuckDB. R/rfscopeR and future **RFScope Lab** (optionally Shiny) consume documented APIs and those files for historical exploration, comparison, reports, clustering, and experiments—never the IQ hot path.

Plugins will begin as versioned data/message contracts. R/Python and risky decoders should be out-of-process; native Rust/C DSP extensions require a separately reviewed ABI and isolation strategy.
