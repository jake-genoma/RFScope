# HackRF backend plan

Milestone 1 does not link or test HackRF. Milestone 2 will place minimal unsafe official `libhackrf` C bindings in a dedicated `rf-device-hackrf` crate behind a Cargo feature and safe `IqSource` wrapper. `pkg-config` will discover the system library while default CI/mock builds remain independent.

Fedora: `sudo dnf install hackrf-devel`. Debian/Ubuntu: `sudo apt install libhackrf-dev hackrf`. USB permissions and udev packaging vary; use distribution and Great Scott Gadgets instructions. Hardware tests will require explicit `RFSCOPE_HARDWARE_TESTS=1`.

The USB callback will only copy/transfer into bounded application-owned buffers and update counters; it will not FFT or demodulate. Errors must distinguish missing library/device, permissions, busy/disconnect, and unsupported settings.

Frequency/rate/filter, generic named gain stages, amplifier/antenna power, clocks, serial and versions will come from reported capabilities where available. Sweep support is capability-driven, including the installed implementation's supported range count. Sweep ranges are sequential/time-multiplexed, never presented as independent tuners.
