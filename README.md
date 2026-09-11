# RFScope

RFScope is an RX-focused, cross-platform SDR workstation foundation: a Rust radio/DSP server, a React workstation with Canvas/WebGL rendering, a Tauri 2 desktop shell, and an R analytics boundary. The receive pipeline supports deterministic mock IQ and optional live HackRF RX. The wider workstation remains in development.

## What works

- Deterministic mock IQ scene (CW, AM, and NFM carriers), FFT/Blackman-Harris PSD in dBFS, and a compact versioned binary WebSocket stream.
- Dark desktop-first live view with spectrum, GPU scrolling waterfall, tuning/rate/FFT/start controls, and diagnostics.
- Capability-oriented source API and one shared `IqSource` pipeline boundary intended for live hardware and SigMF playback.
- Status/state/metrics REST APIs bound to `127.0.0.1`.

## Demo on Linux

Prerequisites: stable Rust, Node.js 20+ and npm. `just` is optional.

```bash
npm install --prefix web
cargo run -p rf-server -- --device mock
# another terminal
npm run dev
# open http://localhost:5173
```

Or use `just demo` and `just dev`. Run `just test` and `just check` for verification. The desktop scaffold uses `npm --prefix apps/desktop install && just desktop`; Tauri additionally needs its [Linux system dependencies](https://v2.tauri.app/start/prerequisites/).

## HackRF status and prerequisites

The optional HackRF control backend uses official Great Scott Gadgets `libhackrf` and has been tested against a locally connected HackRF Pro for enumeration, metadata, open/close, and receiver configuration. Run `cargo run -p rf-server --release --features hackrf` to enable it. Bounded RX streaming feeds the same FFT and binary spectrum protocol as mock IQ. Open a device, then press START RX. See [HackRF integration](docs/hackrf.md) for the explicit hardware diagnostic and verification limits. Fedora development packages are typically installed with `sudo dnf install hackrf-devel`; Debian/Ubuntu use `sudo apt install libhackrf-dev hackrf`. Do not change udev rules blindly—follow distribution/Great Scott Gadgets guidance. The mock build has no libhackrf dependency.

## Architecture

Crates are divided by stable responsibilities rather than tiny placeholder modules: `rf-types`, `rf-device`, `rf-dsp`, `rf-engine`, and `rf-server`. `web` is shared by browser and desktop; `apps/desktop` owns packaging. R is strictly an API/persisted-data consumer. See [architecture](docs/architecture.md), [protocol](docs/protocol.md), [DSP](docs/dsp.md), [HackRF plan](docs/hackrf.md), and [roadmap](docs/roadmap.md).

No spectrum measurements are presented as calibrated dBm: current output is dBFS.
