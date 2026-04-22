# QPlayer

> A cross-platform media playback application for theatre — sound cues, video cues, and show control.

This repository contains a Rust port of the original [C# QPlayer](https://github.com/space928/QPlayer) (WPF / .NET). The goal is full feature parity with the original, plus native cross-platform support (macOS, Windows, Linux) and improved real-time performance.

---

## Features

| Feature | Status | Notes |
|---------|--------|-------|
| **Cue list** (Sound, Stop, Volume, Group, Timecode, Dummy) | ✅ | Full domain model with serde round-trip |
| **Audio playback** | ✅ | WAV, MP3, FLAC, OGG, AIFF, WMA via FFmpeg |
| **Multi-cue mixing** | ✅ | ≥ 64 simultaneous cues, lock-free mixer |
| **Real-time FX** | ✅ | Per-cue EQ (4-band biquad), pan, fade, limiter, metering |
| **Video playback** | ✅ | FFmpeg decode + wgpu fullscreen output, A/V sync via audio clock |
| **Dual-window** | ✅ | Control window (egui) + dedicated video output window |
| **OSC (Open Sound Control)** | ✅ | UDP RX/TX, pattern routing, remote control |
| **MSC (MIDI Show Control over UDP)** | ✅ | MA-MSC packet parsing, device/executor/page filtering |
| **Remote show-file transfer** | ✅ | 1 KB block transfer with ACK/NACK + retry |
| **Plugin architecture** | 🏗️ | WASM sandbox (`wasmtime`) — Phase 6 |
| **Save / Save As** | ✅ | `.qproj` serialization with dirty tracking |
| **Autosave** | ✅ | 5 rotating backups every 60 s |
| **Crash recovery** | ✅ | `human-panic` + SIGINT emergency save |
| **Drag & drop** | ✅ | External audio/video files → new cues |
| **Single instance** | ✅ | Prevents multiple app launches |
| **WASM Plugin host** | ✅ | `wasmtime` sandbox, lifecycle hooks, crash isolation |
| **Undo / redo** | ⏳ | Planned for future session |

---

## Architecture

The Rust workspace is split into focused crates:

```
qplayer-rust/
├── crates/
│   ├── qplayer-core/        # Domain models, serialization, show-file migrations
│   ├── qplayer-audio/       # Real-time audio engine (cpal + FFmpeg decode)
│   ├── qplayer-video/       # wgpu video output + FFmpeg video decoder
│   ├── qplayer-gui/         # egui interface (cue list, inspector, transport)
│   ├── qplayer-protocols/   # OSC, MSC, remote block-transfer
│   ├── qplayer-plugin-api/  # WASM plugin host interface (WIT)
│   └── qplayer/             # Binary — custom winit event loop, wires everything
└── Cargo.toml
```

**Design principles**
- **Pure core** — `qplayer-core` has no I/O, no `std::sync`, no OS deps. It compiles to any target.
- **Lock-free audio** — The mixer callback never allocates or locks. All parameter updates are atomic.
- **A/V sync** — Audio is the master clock. The video decode thread sleeps until `frame.pts <= audio_clock`, then presents via a winit user event.
- **Command dispatch** — GUI, OSC, and MSC all enqueue `AppCommand` variants into a shared queue processed each frame.

---

## Building

### Prerequisites

- **Rust** ≥ 1.85 (2024 edition)
- **FFmpeg** ≥ 8.0 development libraries (`libavcodec`, `libavformat`, `libavutil`, `libswresample`, `libswscale`)
- **Git** LFS (if pulling show-file corpus for regression tests)

#### macOS (Apple Silicon)

```bash
# Install FFmpeg (e.g. via Homebrew)
brew install ffmpeg

# Or if using a custom FFmpeg prefix:
export FFMPEG_DIR=/opt/homebrew/Cellar/ffmpeg-full/8.0.1_3
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig"
```

#### Windows

Install FFmpeg via [vcpkg](https://vcpkg.io/) or [gyan.dev](https://www.gyan.dev/ffmpeg/builds/), then set `FFMPEG_DIR` to the install prefix.

#### Linux

```bash
sudo apt install libavcodec-dev libavformat-dev libavutil-dev \
                   libswresample-dev libswscale-dev pkg-config
```

### Compile

```bash
cd qplayer-rust
cargo build --release
```

The binary is produced at `target/release/qplayer`.

### Run

```bash
# Default: loads with an empty show
cargo run -p qplayer

# Open an existing show file
cargo run -p qplayer -- /path/to/show.qproj
```

---

## Testing

```bash
cd qplayer-rust

# Full workspace test suite
cargo test --workspace

# Individual crates
cargo test -p qplayer-core      # 22 tests — serde round-trip, migrations
cargo test -p qplayer-audio     # 38 tests — mixer, FX chain, decoder
cargo test -p qplayer-gui       #  2 tests — app state, command dispatch
cargo test -p qplayer-protocols #  6 tests — OSC router, MSC parser, block-transfer
```

---

## Project Status

| Phase | Scope | Status |
|-------|-------|--------|
| 0 — Spike | cpal + egui + `.qproj` load | ✅ Complete |
| 1 — Core | Domain models, serde, migrations | ✅ Complete (22 tests) |
| 2 — Audio | Audio engine, FX, FFmpeg decode | ✅ Complete (38 tests) |
| 3 — GUI | egui skeleton, cue list, inspector | ✅ Complete (2 tests) |
| 4 — Integration + Video | A/V sync, dual window, `VideoCue` | ✅ Complete |
| 5 — Protocols | OSC, MSC, remote control | ✅ Complete (6 tests) |
| 6 — Plugins | WASM plugin ABI + port OSC/MagicQ | 🏗️ In progress |
| 6 — Plugins | WASM plugin host with lifecycle hooks, hello-plugin example | ✅ Complete |
| 7 — Polish (partial) | Save, autosave, crash recovery, drag-drop, single instance | 🟡 In Progress |

See [`PORTING_GUIDE.md`](PORTING_GUIDE.md) for the full design rationale, NFR targets, and detailed phase breakdown.

---

## Protocol Reference

### OSC Endpoints

The application listens on UDP port `9000` by default and responds to:

| Address | Arguments | Action |
|---------|-----------|--------|
| `/qplayer/go` | `[qid: float]` (optional) | Go cue (or next if omitted) |
| `/qplayer/stop` | `[qid: float]` (optional) | Stop cue (or all if omitted) |
| `/qplayer/pause` | — | Pause all |
| `/qplayer/unpause` | — | Resume |
| `/qplayer/preload` | `[qid: float]` | Preload cue |
| `/qplayer/select` | `[qid: float]` | Select cue |
| `/qplayer/up` | — | Select previous |
| `/qplayer/down` | — | Select next |
| `/qplayer/save` | — | Save show file |
| `/qplayer/remote/go` | — | Remote go |
| `/qplayer/remote/stop` | — | Remote stop |

Pattern routing supports `?` wildcards (e.g. `/qplayer/?/go`).

### MA-MSC (MIDI Show Control over UDP)

Listens on UDP port `6000`. Parses `GMA\0MSC\0` header + MIDI sysex MSC payloads. Supported commands:

- `Go`, `Stop`, `Resume`, `TimedGo`, `Set`, `Fire`, `GoOff`

Device ID, command format, and executor/page filtering are configurable via `MscManager`.

---

## License

The Rust port is dual-licensed under **MIT OR Apache-2.0** (see individual crate `Cargo.toml` files).

The original C# QPlayer is licensed under the **GPL-3.0** (see `Qplayer-Csharp/LICENSE`).

---

## Credits

- Original C# QPlayer by [Thomas Mathieson](https://github.com/space928)
- Rust port maintained on the `rust-port` branch

---

*Last updated: 2026-04-22 — Phase 5 complete (68 tests passing).*
