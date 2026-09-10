# HackRF RX integration

RFScope uses the official Great Scott Gadgets **libhackrf**, optionally linked by
`rf-device`'s `hackrf` feature. Default/mock builds neither discover nor link it.
The backend implements enumeration, exclusive ownership, metadata, receive
configuration and bounded RX streaming through the common IQ/FFT path. Mock
remains usable without libhackrf. No TX APIs or symbols are bound.

See [current RX verification](rx-validation.md) for measured rates and limitations.
The control-only verification below is historical evidence from the foundation.

## Fedora prerequisites

Install `hackrf` (tools/runtime), `hackrf-devel` (header, linker symlink and
pkg-config file), and `pkgconf-pkg-config` if `pkg-config` is unavailable. Package
installation is a manual administrator action, for example:

```sh
sudo dnf install hackrf hackrf-devel pkgconf-pkg-config
pkg-config --modversion libhackrf
pkg-config --cflags --libs libhackrf
hackrf_info
```

The locally tested Fedora 44 package is 2026.01.3. Its `libhackrf.pc` has a blank
`Version:` field: `--modversion` succeeds with blank output. Library release/API
versions are queried through libhackrf itself. Headers are at
`/usr/include/libhackrf/hackrf.h`; the development linker file is
`/usr/lib64/libhackrf.so`. No bindgen, libclang, or downloaded native driver is
required. Nonstandard installations can set `PKG_CONFIG_PATH`.

If USB access fails, preserve the operation, native error name and numeric code.
Check `hackrf_info` as the same user, USB visibility, distribution udev rules and
whether another application owns the device. Do not automatically install rules
or change permissions. The local host required no udev changes. A restricted
sandbox reported `hackrf_init: Other error (-1000)` while host access succeeded.

## Running

```sh
cargo run -p rf-server --release --features hackrf
npm run dev
```

Use **Refresh devices**, select the full-serial device, then **Open device**.
Metadata and capability-driven controls appear after opening. Edit settings and
press **Apply receiver settings**, then **START RX**. **STOP RX** retains ownership; **Close device** stops and releases it. Close before selecting another device. Selecting
hardware pauses mock production and hides mock plots; switching back to mock
leaves it stopped until **START RX** is pressed. Closing clears applied settings;
reopening reapplies conservative defaults (100 MHz, 8 MS/s, 5 MHz filter, all gains
zero, RF amplifier and antenna power disabled).

The server always binds localhost. For an isolated second instance, set
`RFSCOPE_PORT=8788`; start Vite with
`VITE_RFSCOPE_API=http://127.0.0.1:8788/api/v1 npm --prefix web run dev -- --port 5174`.
The original defaults remain ports 8787 and 5173.

## Ownership and FFI

Private manual declarations in `crates/rf-device/src/hackrf/ffi.rs` match the
installed official 2026.01.3 header. Unsafe calls and raw pointers remain inside
that backend. A safe generic `ReceiverControl` seam separates low-rate ownership
from `IqSource`; a bounded source mailbox now supplies the real IQ adapter.

A dedicated thread exclusively owns the native context and receiver. Its control
queue is bounded at eight requests and rejects saturation; HTTP dispatch uses
`spawn_blocking`. An explicit reference-counted context outlives every handle.
A process lifecycle guard prevents concurrent libhackrf initialization; it stores
no SDR configuration. RAII handles error-path cleanup; explicit close/shutdown
reports native failures. No unsafe Send/Sync implementation is used. Selection is
by a complete enumerated serial, never libhackrf's ambiguous suffix matching.
USB product identity alone cannot distinguish a Pro: board metadata is queried
after opening. Devices without readable serials and unreviewed board profiles
produce an explicit error rather than ambiguous ownership or invented controls.

Complete configurations are validated before any setters run. Native setters
are not transactional: a native failure closes ownership and clears applied
settings rather than claiming rollback. Values returned by the API are settings
accepted by the library, **not hardware readback or measured RF calibration**.
Sample rate is set before frequency and explicit filter bandwidth, since changing
rate resets the filter. Antenna power is kept disabled; it has no public control.

## Capabilities

The board ID, serial, part ID, hardware revision (where firmware supports it),
firmware string/API, and library release/API are queried. libhackrf has no generic
range/gain discovery API. The backend therefore reports an explicit documented
compatibility profile selected by the queried board ID:

- HackRF Pro (5): 100 kHz–6 GHz operating range.
- HackRF One (2/4): 1 MHz–6 GHz. Not hardware-tested in this milestone.
- Conventional sample-rate configuration: 2–20 MS/s. Extended-precision and
  40 MS/s modes are not exposed.
- Named IF gain: 0–40 dB, step 8; baseband gain: 0–62 dB, step 2.
- Named RF amplifier control: 0/1 enable, not a calibrated dB value.
- Filter choices come from libhackrf's round-down helper, not a duplicated table.
  Configuration rejects a filter wider than sample rate.

These are RFScope-supported settings, not a claim of the hardware's full limits.
Generic types/UI contain no assumptions about gain-stage names. No power is
reported as calibrated dBm.

References: [official header](https://github.com/greatscottgadgets/hackrf/blob/v2026.01.3/host/libhackrf/src/hackrf.h),
[HackRF Pro specifications](https://hackrf.readthedocs.io/en/latest/hackrf_pro.html).

## Explicit hardware diagnostic

Ordinary tests, including feature-enabled tests, do not initialize USB or open a
device. Run the diagnostic explicitly with a complete serial, after closing any
RFScope/UI or external hardware owner:

```sh
cargo run -p rf-device --features hackrf --example hackrf-diagnostic -- \
  0000000000000000977c64de21718a13
```

It initializes libhackrf, enumerates, opens, queries metadata, applies two sets of
frequency/rate/gain/filter controls, closes, reopens, closes, and checks
`hackrf_exit`. It never starts RX or TX. A native error fails the command.

Locally verified on 2026-09-10 through RFScope:

- HackRF Pro, board ID 5, r1.2, serial `0000000000000000977c64de21718a13`.
- Firmware `n_260808`, API `1.13`; library release `2026.01.3`, API `0.9.2`.
- Part ID `a0000a30 006a4775`.
- 100 MHz / 8 MS/s / IF 16 dB / baseband 20 dB / filter 5 MHz.
- 101 MHz / 10 MS/s / IF 24 dB / baseband 24 dB / filter 7 MHz.
- HTTP API additionally verified 102 MHz / 10 MS/s / IF 24 dB / baseband
  22 dB / filter 7 MHz, invalid configuration rejection, ownership conflict,
  idempotent close, reopen, and return to mock.
- Explicit library shutdown (`hackrf_exit`) passed after close/reopen.
- RF amplifier disabled and antenna power disabled; enabling the amplifier was
  not tested. Setters succeeded; RF tuning accuracy was not measured.

`hackrf_info` reported three other devices sharing the USB bus: retain this as a
potential high-rate performance consideration. No USB configuration was changed.

## Software verification

Passed `cargo fmt --all -- --check`, `cargo test --workspace` (7 tests),
`cargo clippy --workspace --all-targets -- -D warnings`, `npm --prefix web test`
(1 test), `npm --prefix web run check`, and `npm --prefix web run build`.
Feature-enabled workspace tests (8 tests) and Clippy also passed with
`--features rf-server/hackrf`. Default and feature-enabled server builds passed;
`ldd` confirmed the default server has no libhackrf dependency. A local C11
compile-only check validated the manual device-list layout, serial struct size,
and key function signatures against the installed header.

Visual UI interaction was not verified because browser automation had no available
browser. Physical unplug/reconnect, amplifier-enabled operation and other boards
were not tested. The API tests and diagnostic used a separate local server/device
session; no system packages, udev rules or USB configuration were changed.

## RX lifecycle and overload

Configuration changes while receiving stop RX, apply all settings and restart
with a fresh pool. Even frequency-only changes currently use this conservative
restart: tuning is supported during operation, with a brief intentional gap.
Counters reset for each RX source. Native-stream failures stop delivery and
increment `stream_faults`; explicit restart or close/reopen allows recovery.
Raw USB samples have no sequence number, so application drop counters cannot
prove that firmware/USB never lost samples. Physical unplug is not verified.
