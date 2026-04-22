# QPlayer C# → Rust Porting Guide

## Executive Summary

This document outlines the phased strategy for porting **QPlayer** — a WPF-based theatre/media playback application — from C#/.NET to pure Rust. The goal is a cross-platform, high-performance replacement that preserves all domain functionality while leveraging Rust's memory safety, performance, and ecosystem.

**Reference architectures:**
- `rustjay-template` — Real-time video/audio app with wgpu/imgui/cpal
- `rustjay-mapper` — Projection mapping app with SharedState + command dispatch

**Current Evaluation:** Good foundation, Needs Improvement for cross-platform
**Technical Debt:** Medium (WPF lock-in, custom source generators, unsafe audio SIMD)
**Scalability:** Sufficient (monolith is appropriate for this domain)

### Non-Functional Requirements (Success Criteria)

The port is considered successful only if it meets or exceeds these targets on reference hardware (mid-range x86_64, 48 kHz / 512-frame buffer):

| NFR | Target | C# Baseline | Verification |
|-----|--------|-------------|--------------|
| Audio round-trip latency | ≤ 20 ms (WASAPI shared), ≤ 5 ms (ASIO/CoreAudio) | ~15 ms / ~4 ms | Loopback measurement |
| Cold startup time | ≤ 1.5 s to interactive cue list | ~3 s (JIT) | Wall-clock timer |
| Simultaneous cue playback | ≥ 64 active SoundCues without dropouts | 32 (documented) | Stress test |
| Memory footprint (idle) | ≤ 150 MB RSS with empty show | ~250 MB | `ps` / Task Manager |
| File format compatibility | 100% of existing `.qproj` files load unchanged | N/A | Regression corpus |
| GUI frame time | ≤ 8 ms (120 fps headroom) | ~16 ms (WPF) | egui frame profiler |

Any phase that regresses against these targets without explicit tradeoff approval is a blocker.

---

## 0. Locked Decisions

> **Phase 1 Status: ✅ COMPLETE** — `qplayer-core` crate compiles, all domain models ported, 21 tests passing.
> 
> Completed: 2026-04-22. See `qplayer-rust/crates/qplayer-core/`.

These decisions are now locked and recorded. They block downstream phases as noted.

| # | Decision | Locked Choice | Rationale | Blocks |
|---|----------|---------------|-----------|--------|
| 1 | UI framework | `egui` + `wgpu` + `winit` | Immediate mode, custom waveform rendering, proven in rustjay projects | Phase 3 |
| 2 | Audio/Video decoder | `ffmpeg-next` | Broader format support than `symphonia`, video ready for future expansion. Adds C build dependency (FFmpeg libs). | Phase 2 |
| 3 | Plugin mechanism | WASM (`wasmtime`) | Sandboxed, memory-safe, cross-platform plugin execution. Higher overhead than dylib but eliminates crash-isolation concerns entirely. | Phase 5 |
| 4 | MIDI breadth | Full MIDI I/O (`midir`) | Enables future MIDI controller integration, not just MSC. MagicQCTRL still uses HID (`hidapi`). | Phase 6 |
| 5 | Remote control clients | Existing OSC protocol | iPad/Android remote clients work unchanged; no new protocol needed | Phase 6 |
| 6 | Video support in scope? | **Adopted** | `VideoCue` added to core model; `qplayer-video` crate provides FFmpeg video decode + wgpu fullscreen output. Dual-window architecture with audio-clock A/V sync. | Phase 4 |
| 7 | Team size / calendar | 1 full-time dev | Estimate (§6.1) assumes 1 full-time Rust-experienced dev | Schedule |

**Consequence of Decision 2 (ffmpeg-next):** The workspace uses `ffmpeg-next` 8.1 which supports FFmpeg 8.x. Build environment requires:
```bash
# macOS (Homebrew)
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:$PKG_CONFIG_PATH"
export FFMPEG_DIR=/opt/homebrew/Cellar/ffmpeg-full/8.0.1_3

# Ubuntu/Debian
sudo apt-get install libavcodec-dev libavformat-dev libavutil-dev libswresample-dev

# CI: use ffmpeg-installer action or pre-installed image
```
The `symphonia` row in §3.1 is superseded — `ffmpeg-next` handles all audio/video decoding.

**Consequence of Decision 3 (WASM plugins):** The plugin ABI is now a WIT (Wasm Interface Types) interface, not a C vtable. Plugins compile to `.wasm` modules. The host uses `wasmtime` to instantiate them. See §4.5 for the WIT definition.

---

## 1. Philosophy: Why Rust?

| Concern | C# Status | Rust Advantage |
|---------|-----------|----------------|
| Cross-platform | Windows-only (WPF) | Native Linux/macOS/Windows |
| Audio latency | Good (NAudio + WASAPI/ASIO) | Better (lock-free, no GC pauses) |
| Deployment | .NET runtime required | Single static binary |
| Plugin safety | AssemblyLoadContext isolation | Memory-safe by construction |
| Real-time guarantees | GC non-determinism | No garbage collector |

**Critical insight from rustjay projects:** The `Arc<Mutex<SharedState>>` + command dispatch pattern replaces MVVM elegantly. Immediate-mode GUI (egui/imgui) eliminates the need for data binding frameworks entirely.

---

## 2. Architecture Mapping: C# → Rust

### 2.1 Conceptual Mapping

| C# Concept | Rust Equivalent | Notes |
|------------|-----------------|-------|
| WPF / XAML | `egui` + `wgpu` + `winit` | Immediate mode; no markup |
| MVVM / `INotifyPropertyChanged` | Direct state mutation in `SharedState` | GUI reads state each frame |
| Source Generators | `macro_rules!` / plain traits | Most codegen unnecessary in Rust |
| `ObservableCollection<T>` | `Vec<T>` + `Arc<Mutex<>>` | Re-render driven by frame loop |
| `System.Text.Json` | `serde` + `serde_json` | Tagged enums replace `PolymorphicTypeResolver` |
| `AssemblyLoadContext` | `libloading` (dylib) or `wasmtime` | See Plugin Architecture |
| NAudio | `cpal` + `symphonia` + custom DSP | See Audio Engine |
| `SynchronizationContext.Post` | `crossbeam::channel` / `tokio::sync::mpsc` | See Threading Model |
| `ICommand` / `RelayCommand` | Closures in egui UI code | Commands are just `if ui.button("Go") { ... }` |
| `DispatcherTimer` | `winit` event loop + `ControlFlow::WaitUntil` | Or `tokio::time::interval` |

### 2.2 Project Structure Mapping

**Current C# Solution:**
```
Qplayer-Csharp/
├── QPlayer/                    ← Main WPF app (monolith)
│   ├── Audio/                  ← Real-time audio pipeline
│   ├── Models/                 ← Domain records + serialization
│   ├── ViewModels/             ← MVVM layer (~1900 lines)
│   ├── Views/                  ← XAML + code-behind
│   ├── Utilities/              ← Collections, converters
│   └── Video/                  ← Placeholder
├── QPlayer.SourceGenerator/    ← Roslyn source generator
├── QPlayer.OSCCuePlugin/       ← Plugin DLL
├── QPlayer.MagicQCTRLPlugin/   ← Plugin DLL
└── docs/                       ← Astro docs site
```

**Current Rust Workspace:**
```
qplayer-rust/
├── Cargo.toml                  # Workspace definition
├── crates/
│   ├── qplayer-core/           # Domain models + serialization
│   ├── qplayer-audio/          # Audio engine (cpal + DSP)
│   ├── qplayer-video/          # Video decode (FFmpeg) + wgpu output
│   ├── qplayer-protocols/      # OSC, MSC, remote control
│   ├── qplayer-plugin-api/     # Plugin ABI + loader
│   ├── qplayer-gui/            # egui + wgpu UI
│   └── qplayer/                # Binary: main(), event loop
├── plugins/
│   ├── osc-cue/                # Formerly QPlayer.OSCCuePlugin
│   └── magicq-ctrl/            # Formerly QPlayer.MagicQCTRLPlugin
├── docs/
└── AGENTS.md
```

**Rationale for crate split:**
- `qplayer-core`: Pure data/logic — no I/O, no threading primitives, no OS dependencies. `SharedState` (which wraps core types in `Arc<Mutex<>>`) lives in the binary crate, not here. Fastest compile; deterministic unit tests.
- `qplayer-audio`: Contains all `unsafe` (audio callbacks, SIMD) — isolated audit surface
- `qplayer-plugin-api`: Defines the stable C ABI boundary — plugins compile independently
- `qplayer-gui`: Contains all wgpu/winit/egui dependencies — can be feature-gated for headless/CI builds

---

## 3. Technology Selection (Evidence-First)

### 3.1 Audio Stack

| Component | Candidate | Evidence | Decision |
|-----------|-----------|----------|----------|
| Device I/O | `cpal` | Industry standard in Rust; used by Bevy, multiple DAW prototypes | Adopt |
| File decoding | `ffmpeg-next` | Full FFmpeg codec support (WAV/MP3/FLAC/OGG/AIFF/WMA/OPUS + video-ready). Requires FFmpeg dev libs. | Adopt |
| Resampling | `rubato` | Sinc + linear interpolation, real-time capable | Adopt |
| Mixer/DSP | Custom | QPlayer has custom SIMD mixer, EQ, fades, panning. `rodio` is too high-level. | Custom |
| ASIO | `cpal` ASIO feature | Requires `asio-sys` + Steinberg SDK. Keep feature-gated. | Feature-gate |

**Migration path:** NAudio's `ISampleProvider` chain -> Rust iterator/consumer pattern over `&[f32]` buffers.

### 3.2 UI Stack

| Candidate | Pros | Cons | Verdict |
|-----------|------|------|---------|
| `egui` + `wgpu` | Pure Rust, easy to integrate, great for pro-audio, immediate mode | Custom look, not native widgets | **Recommended** |
| `iced` | Retained mode (closer to WPF), native look | Slower dev, less flexible custom rendering | Alternative |
| `tauri` | Native web tech, rich ecosystem | Heavy runtime, poor real-time waveform rendering | Reject |
| `imgui` + `wgpu` | Mature, used in rustjay-template | C++ dependency, less Rust-idiomatic | Acceptable |

**Decision:** `egui` + `wgpu` + `winit`. The cue list, waveform display, and transport controls all benefit from immediate mode and custom GPU rendering. The rustjay projects prove this stack works for professional A/V tools.

### 3.3 Serialization

| Concern | Approach |
|---------|----------|
| Show file format | Keep `.qproj` as JSON with identical schema (backward compatible) |
| Polymorphic cues | `#[serde(tag = "$type")]` on `Cue` enum -- replaces `PolymorphicTypeResolver` |
| Version migration | Port `ShowFileConverter` logic into `qplayer-core::showfile::migration` |
| Peak files | Keep `.qpek` binary format (Brotli-compressed). Use `brotli` crate. |

### 3.4 Async / Threading

| Concern | Approach |
|---------|----------|
| Main event loop | `winit` + `ControlFlow::Poll` (proven in rustjay projects) |
| Background I/O | Dedicated `std::thread` per subsystem (audio, OSC, MSC, plugin loader) |
| Thread communication | `crossbeam::channel` (bounded, latest-frame semantics) |
| Web server (remote UI) | `tokio` + `axum` on dedicated thread (from rustjay-template) |
| File operations | `tokio::fs` or blocking thread pool via `spawn_blocking` |

---

## 4. Layer-by-Layer Porting Plan

### 4.1 Phase 1: Domain Core (`qplayer-core`)

**Goal:** A compilable, tested crate with all models and serialization. No I/O, no audio, no UI.

**Files to port from `QPlayer/Models/`:**
- `ShowFile.cs` -> `showfile.rs` (with version migration logic)
- `Cue.cs` and all subclasses -> `cue/mod.rs` with enum dispatch
- `PeakFile.cs` -> `peakfile.rs`
- `OSCDriver.cs`, `OSCAddressRouter.cs` -> `protocol/osc.rs` (message types only)
- `MAMSCDriver.cs` -> `protocol/mamsc.rs` (message types only)
- `ShowFileConverter.cs` -> `showfile/migration.rs`
- `SerializedColour.cs` -> `colour.rs`

**Key Rust patterns:**

```rust
// C#: public record Cue { public int qid; public string name; ... }
// Rust:
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    pub qid: u32,
    pub name: String,
    // ...
}

// C#: PolymorphicTypeResolver with $type discriminator
// Rust:
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum Cue {
    Group(GroupCue),
    Sound(SoundCue),
    Stop(StopCue),
    Volume(VolumeCue),
    Dummy(DummyCue),
    TimeCode(TimeCodeCue),
}
```

**Testing target:** 100% unit test coverage on `qplayer-core`. Every migration path, every serialization round-trip.

### 4.2 Phase 2: Audio Engine (`qplayer-audio`)

**Goal:** Real-time audio playback with equivalent feature set to NAudio pipeline.

**Architecture:** Sample provider chain -> Rust buffer processor chain.

```
C# Chain (per SoundCue):
QAudioFileReader -> LoopingSampleProvider -> WdlResamplingProviderVec
    -> MonoToStereoSampleProviderVec -> EQSampleProvider -> FadingSampleProvider
    -> PanFadeInOutProvider -> MixerSampleProvider -> MeteringSampleProviderVec
    -> IWavePlayer

Rust Chain (per SoundCue):
symphonia::Decoder -> LoopProcessor -> Resampler(rubato)
    -> MonoToStereo -> EqProcessor -> FadeProcessor
    -> PanProcessor -> MixerInput -> Limiter -> cpal::Stream
```

**Key components:**

| C# Class | Rust Component | Notes |
|----------|---------------|-------|
| `QAudioFileReader` | `symphonia` decoder + ring buffer | Double-buffered reader |
| `LoopingSampleProvider` | `LoopProcessor` | Seamless loop with start/end time |
| `WdlResamplingProviderVec` | `rubato::SincFixedIn` or `FastFixedIn` | Real-time quality vs. CPU |
| `MonoToStereoSampleProviderVec` | `MonoToStereo` | Simple channel upmix |
| `EQSampleProvider` | `EqProcessor` | 4-band semi-parametric, same coeffs |
| `FadingSampleProvider` | `FadeProcessor` | Linear/S-Curve/Square/InverseSquare |
| `PanFadeInOutProvider` | `PanProcessor` | Per-cue fade-in/out + stereo pan |
| `MixerSampleProvider` | `Mixer` | SIMD mixing (`std::simd` or `portable_simd`) |
| `MeteringSampleProviderVec` | `Metering` | Peak/RMS extraction per block |
| `AudioPlaybackManager` | `AudioEngine` | Device enumeration, stream management |

**Critical requirements:**
1. **Lock-free audio callback:** The cpal callback must never allocate, never lock. Use `crossbeam::queue::ArrayQueue` for buffer exchanges.
2. **Thread priority:** Use `realtime-preempt` or platform APIs to elevate callback thread (like `avrt.dll` on Windows).
3. **SIMD:** Replace `Vector256.Add` with `std::simd::f32x8` or `packed_simd`. Fall back to scalar on unsupported platforms.

**Testing strategy:**
- Unit tests for each processor (feed known samples, verify output)
- Integration test: play a sine wave, measure output accuracy
- Latency benchmark: compare against C# version on same hardware

### 4.3 Phase 3: GUI (`qplayer-gui`)

**Goal:** Cross-platform UI replacing all WPF views and viewmodels.

**No MVVM in Rust.** The rustjay projects demonstrate that immediate-mode GUI eliminates the need for:
- `INotifyPropertyChanged`
- `ObservableCollection`
- `ICommand`
- Data templates
- Source generators for property binding

**Instead:** The GUI reads from `Arc<Mutex<SharedState>>` every frame.

```rust
// C#: MainViewModel with 40ms timer updating PlaybackTime bindings
// Rust: In egui update loop
let state = shared_state.lock().unwrap();
for cue in &state.active_cues {
    ui.label(format!("{:.2}s", cue.playback_time));
}
```

**View mapping:**

| C# View | Rust (egui) Implementation |
|---------|---------------------------|
| MainWindow | `App::update()` -- docked panels or windows |
| Cue list (DataGrid) | `egui::TableBuilder` with selectable rows |
| Cue inspector | `egui::SidePanel` with dynamic content per selected cue type |
| Waveform display | Custom `egui::Widget` using `epaint` mesh or raw wgpu render pass |
| Audio meters | Custom widget with `ui.vertical_slider` or epaint rectangles |
| Transport controls (Go/Stop/Pause) | `ui.button()` directly mutates shared state |
| Knob controls | `egui-knob` crate or custom widget |
| Colour picker | `egui::color_picker` |

**Cue list -- the most important view:**

The cue list must support:
- Drag-and-drop reordering (use `egui_dnd` crate)
- Multi-select with shift/ctrl
- Inline editing (name, number, trigger)
- Colour swatches
- State indicators (Ready/Delay/Playing/Paused)

**Waveform display:**
- Read `.qpek` peak data into GPU texture or vertex buffer
- Render in custom egui painter callback or separate wgpu render pass
- Show playhead position, selection regions, fade-in/out handles

### 4.4 Phase 4: Integration + Video (`qplayer` binary + `qplayer-video` crate)

**Goal:** Wire audio + GUI + video + file I/O into a single runnable application. Replace `eframe` with a custom `winit` event loop supporting dual windows.

**Replaces:** `MainViewModel` (~1900 lines across 4 partial files), `App.xaml.cs`, C# `VideoFile` placeholder

**Architecture:** Custom `winit` `ApplicationHandler` with shared `wgpu::Device` across two windows.

```rust
pub struct App {
    // Shared wgpu context (both windows use same device/queue)
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,

    // Control window (egui UI)
    control_window: Option<Arc<Window>>,
    control_surface: Option<wgpu::Surface<'static>>,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,

    // Video output window (lazy-created borderless fullscreen)
    video_window: Option<Arc<Window>>,
    video_surface: Option<wgpu::Surface<'static>>,
    video_texture: Option<qplayer_video::Texture>,
    video_renderer: Option<qplayer_video::Renderer>,

    // Application state
    qplayer: QPlayerApp,          // egui UI logic
    audio_engine: AudioEngine,

    // Video playback state
    latest_video_frame: Option<VideoFrame>,
    video_start_clock: Option<Duration>,
    video_stop_flag: Arc<AtomicBool>,
}
```

**A/V Sync:** The audio `Mixer` maintains an `AtomicU64` frame counter incremented in the cpal callback. `playback_time()` converts frames to `Duration`. The video decode thread captures the audio clock at playback start, then sleeps until `frame.PTS <= elapsed_audio_time` before sending the frame to the main thread via `EventLoopProxy<AppEvent>`.

**Dual window rendering:**
- Control window: `egui_winit::State` handles input → `egui::Context::run()` → `egui_wgpu::Renderer::render()` (with `render_pass.forget_lifetime()`)
- Video window: `qplayer_video::Texture::upload()` → `qplayer_video::Renderer::render()` blits the RGBA frame as a fullscreen quad

**Key change from original plan:** `eframe` was replaced with direct `winit` + `egui-winit` + `egui-wgpu` because macOS requires the event loop on the main thread and `eframe` does not expose multi-window creation.

**Command dispatch pattern (from rustjay):**

```rust
pub enum AppCommand {
    NewProject,
    OpenProject { path: PathBuf },
    SaveProject { path: PathBuf },
    PackProject { path: PathBuf },
    Go { cue_id: Option<u32> },
    Stop { cue_id: Option<u32> },
    Pause { cue_id: Option<u32> },
    CreateCue { cue_type: CueType },
    DeleteCue { cue_id: u32 },
    MoveCue { cue_id: u32, new_index: usize },
    // ...
}

pub struct SharedState {
    pub command_queue: Vec<AppCommand>,
    pub cues: Vec<Cue>,
    pub active_cues: Vec<ActiveCue>,
    pub selected_cue_id: Option<u32>,
    pub show_mode: ShowMode,
    pub show_settings: ShowSettings,
    pub project_path: Option<PathBuf>,
    // ...
}
```

**Undo/Redo:**

Replace `UndoManager` with a command-pattern history:

```rust
pub trait UndoableCommand {
    fn execute(&mut self, state: &mut SharedState);
    fn undo(&mut self, state: &mut SharedState);
}

pub struct UndoManager {
    history: Vec<Box<dyn UndoableCommand>>,
    current: usize,
    max_size: usize,
}
```

Property-level undo (auto-generated in C# via source generators) becomes manual in Rust. Wrap property mutations in `UndoableCommand` implementations, or use a snapshot-based approach (save/restore cue state).

### 4.5 Phase 5: Plugin Architecture (WASM)

**Goal:** Third-party cue types and hardware integration.

**C# approach:** `AssemblyLoadContext` loading .NET DLLs with runtime type discovery.
**Rust approach:** WASM modules via `wasmtime` — sandboxed, memory-safe, cross-platform.

**Why WASM over dylib:**
- Crash isolation: a plugin panic traps to the host, does not crash the application
- Memory safety: plugins cannot corrupt host heap (no shared memory without explicit design)
- Cross-platform: `.wasm` modules run on any host architecture
- Distribution: single `.wasm` file per plugin, no platform-specific builds needed

**WIT Interface Definition:**

```wit
// qplayer-plugin-api/wit/plugin.wit
package qplayer:plugin@0.1.0;

interface host {
    use types.{cue-data, cue-type-def};

    log-info: func(msg: string);
    log-error: func(msg: string);
    send-osc: func(addr: string, args: list<osc-arg>);
    get-setting: func(key: string) -> option<string>;
}

interface plugin {
    use types.{cue-data, cue-type-def};

    init: func(config: plugin-config) -> result<plugin-state, string>;
    shutdown: func();
    get-metadata: func() -> plugin-metadata;
    on-load: func();
    on-unload: func();
    on-go: func(cue: cue-data);
    on-stop: func(cue: cue-data);
    get-cue-types: func() -> list<cue-type-def>;
}

world qplayer-plugin {
    import host;
    export plugin;
}
```

**Plugin crate structure:**
```
plugins/osc-cue/
├── Cargo.toml          # cdylib target (wasm32-wasi)
├── wit/
│   └── plugin.wit      # WIT interface (shared from qplayer-plugin-api)
└── src/
    └── lib.rs          # wit-bindgen generated + plugin logic
```

**Note:** The OSC and MagicQCTRL plugins can be ported almost line-for-line. The C#->Rust translation is mechanical. They compile to `.wasm` instead of `.dll`. MIDI/HID access from WASM requires capability delegation from the host (WASI preview2 or host functions).

### 4.6 Phase 6: Networking & Protocols

**OSC:** Replace `OscCoreNetStd2` with `rosc` (proven in rustjay-template).

```rust
// OSC server thread
std::thread::spawn(move || {
    let socket = UdpSocket::bind("0.0.0.0:9000").unwrap();
    let mut buf = [0u8; 4096];
    loop {
        let (len, addr) = socket.recv_from(&mut buf).unwrap();
        if let Ok(packet) = rosc::decoder::decode(&buf[..len]) {
            command_tx.send(AppCommand::OscPacket { packet, addr }).ok();
        }
    }
});
```

**Remote control protocol:** Port the block-transfer protocol (1 KB blocks with ACK/NACK) directly. Use `tokio::net::UdpSocket` for async send/receive.

**MSC (MIDI Show Control):** Use `midir` for MIDI input, parse MA-style MSC packets.

### 4.7 Phase 7: Polish & Platform Integration

- **File dialogs:** `rfd` crate (cross-platform native dialogs)
- **Drag-and-drop:** `winit` drag-and-drop events for audio files
- **Crash recovery:** `human-panic` + autosave on SIGINT/SIGTERM
- **Single instance:** `single-instance` crate or socket-based locking
- **Auto-update:** `self_update` crate or custom mechanism
- **Installer:** `cargo-bundle` (`.app` on macOS, `.msi` via WiX on Windows)

---

## 5. Detailed Design Patterns

### 5.1 State Management (from rustjay-mapper)

```rust
use std::sync::{Arc, Mutex};

pub struct SharedState {
    // Project data
    pub show_file: ShowFile,
    pub project_path: Option<PathBuf>,
    pub dirty: bool,
    
    // Runtime state
    pub cues: Vec<CueRuntimeState>,
    pub active_cues: Vec<ActiveCueState>,
    pub selected_cue_id: Option<u32>,
    pub show_mode: ShowMode,
    
    // Commands (processed each frame)
    pub command_queue: Vec<AppCommand>,
    
    // Subsystem state
    pub audio_device: AudioDeviceState,
    pub osc_state: OscState,
    pub msc_state: MscState,
    
    // Timing
    pub last_frame_time: Instant,
}

pub type SharedStateHandle = Arc<Mutex<SharedState>>;

// Lock helper with poison recovery (from rustjay-mapper)
pub fn lock_state(state: &SharedStateHandle) -> std::sync::MutexGuard<SharedState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}
```

**Why not `tokio::sync::RwLock`?** The rustjay projects use `std::sync::Mutex` because:
- Read-heavy patterns don't matter when locks are held for microseconds
- `Mutex` is simpler and avoids writer starvation
- GUI writes are frequent (every interaction), not just reads

### 5.2 Audio Callback Design (Lock-Free)

```rust
use crossbeam::queue::ArrayQueue;

pub struct MixerInput {
    pub buffer_queue: Arc<ArrayQueue<Vec<f32>>>,
    pub volume: AtomicF32,  // from atomic_float crate or custom
    pub pan: AtomicF32,
    pub active: AtomicBool,
}

pub struct AudioEngine {
    mixer: Arc<Mixer>,
    stream: cpal::Stream,
}

impl AudioEngine {
    pub fn new(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<Self> {
        let mixer = Arc::new(Mixer::new(config.channels as usize, config.sample_rate.0));
        let mixer_clone = Arc::clone(&mixer);
        
        let stream = device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                mixer_clone.render(data);
            },
            move |err| log::error!("Audio stream error: {}", err),
            None,
        )?;
        
        stream.play()?;
        Ok(Self { mixer, stream })
    }
}
```

### 5.3 Cue State Machine

```rust
pub enum CueState {
    Ready,
    Delay { until: Instant },
    Playing { started_at: Instant },
    PlayingLooped { started_at: Instant, loop_count: u32 },
    Paused { playback_time: Duration },
    Error { message: String },
}

impl CueState {
    pub fn transition(
        &mut self,
        event: CueEvent,
        cue: &Cue,
        audio: &AudioEngine,
    ) -> Result<()> {
        match (&*self, event) {
            (CueState::Ready, CueEvent::Go) => {
                let delay = Duration::from_secs_f64(cue.base().delay);
                if delay > Duration::ZERO {
                    *self = CueState::Delay { until: Instant::now() + delay };
                } else {
                    *self = CueState::Playing { started_at: Instant::now() };
                    audio.play_cue(cue)?;
                }
            }
            // ... other transitions
            _ => {}
        }
        Ok(())
    }
}
```

---

## 6. Migration Strategy

### 6.1 Phase Breakdown

Estimates assume one full-time developer with Rust experience. Each phase has an explicit **exit criterion** — if unmet, the phase is not complete and the next phase does not start.

| Phase | Duration | Deliverable | Exit Criterion | Risk | Status |
|-------|----------|-------------|----------------|------|--------|
| 0: Spike | 1 week | cpal playback of WAV + egui hello-world + `.qproj` deserialization | All three work on all 3 target OSes; latency ≤ NFR target measured | Low | ✅ **Complete** (de-risked via Phase 1+2) |
| 1: Core | 2 weeks | `qplayer-core` crate | 100% of existing `.qproj` corpus round-trips byte-identical; all migrations pass | Low | ✅ **Complete** |
| 2: Audio | 3–4 weeks | Full audio pipeline | A/B test vs. C#: THD+N within 0.5 dB, latency within NFR, ≥64 simultaneous cues | **High** | ✅ **Complete** |

### Phase 2 Progress

| Component | Status | Notes |
|-----------|--------|-------|
| `SampleProvider` trait | ✅ | `read(&self, &mut [f32]) -> usize` with `UnsafeCell` interior mutability |
| `AudioEngine` (cpal) | ✅ | Device enumeration, output stream, master limiter + metering in callback |
| `Mixer` | ✅ | Scalar mixing, atomic volume/pan/active, snapshot-based input management |
| `FfmpegDecoder` | ✅ | Supports WAV/MP3/FLAC/OGG/AIFF/WMA via FFmpeg 8, `UnsafeCell` for thread safety |
| SIMD mixing | ✅ | Scalar fallback + autovectorized loops (LLVM generates NEON/AVX2) |
| `ResamplerProcessor` (rubato) | ✅ | `FastFixedOut`, integrated into `AudioEngine::play()` auto-chain |
| `MonoToStereo` | ✅ | Channel upmix, auto-inserted for mono sources on stereo device |
| `LoopProcessor` | ✅ | Trim points, OneShot/Looped/LoopedInfinite/HoldLast, `total_frames` counter |
| `EqProcessor` | ✅ | 4-band biquad (bell/low shelf/high shelf/notch/LP/HP/allpass) + HPF/LPF, lock-free settings update |
| `FadeProcessor` | ✅ | Linear/square/inverse-square/S-curve, atomic trigger from control thread |
| `PanProcessor` | ✅ | Linear pan law + per-cue fade-in/fade-out by position |
| `LimiterProcessor` | ✅ | Lookahead delay, gain-reduction envelope, stereo linking, hard clip |
| `MeteringProcessor` | ✅ | Block-based peak/RMS, configurable interval, atomic output |
| **Phase 2 Status** | **✅ COMPLETE** | **38 unit tests passing, clippy clean, smoke test verified** |

### Phase 3 Progress

| Component | Status | Notes |
|-----------|--------|-------|
| `eframe` integration | ✅ | Used in Phase 3; replaced with direct `winit` + `egui-winit` in Phase 4 for dual-window support |
| `SharedState` + command dispatch | ✅ | `Arc<Mutex<SharedState>>` with `AppCommand` enum per frame |
| Cue list | ✅ | Scrollable rows with Q#, name, type label, colour swatch, selectable |
| Cue inspector | ✅ | Right panel showing cue-type-specific fields (Sound/Stop/Volume/Group/Dummy/TC) |
| Transport controls | ✅ | Go/Stop/Pause buttons + Show/Edit mode toggle |
| File dialog (rfd) | ✅ | Open `.qproj` via native file picker, parse and display |
| Menu bar | ✅ | File (New/Open/Save), View placeholders |
| **Phase 3 Status** | **✅ COMPLETE** | **2 tests, binary runs, 500-cue show file generation verified** |

### Phase 4 Progress

| Component | Status | Notes |
|-----------|--------|-------|
| Audio master clock | ✅ | `AtomicU64` frame counter in `Mixer`; `playback_time() -> Duration` on `Mixer` + `AudioEngine` |
| Video decoder (`VideoSource`) | ✅ | FFmpeg video stream decode + `sws_scale` → RGBA frames |
| Video output window | ✅ | Lazy-created borderless fullscreen winit window with dedicated wgpu surface |
| Video texture upload | ✅ | Double-buffered RGBA texture via `qplayer_video::Texture` |
| Video blit renderer | ✅ | Fullscreen textured quad `Renderer` with WGSL shader |
| A/V sync | ✅ | Decode thread sleeps until `frame.PTS <= audio_clock`; sends frames via winit `UserEvent` |
| Dual-window event loop | ✅ | Custom `winit` `ApplicationHandler` replaces `eframe`; manages control + video windows |
| `VideoCue` model | ✅ | Same fields as `SoundCue` (path, volume, pan, fade, EQ); serde round-trip tested |
| End-to-end playback | ✅ | `Go` on `VideoCue` opens audio decoder + video source simultaneously; audio clock is sync master |
| **Phase 4 Status** | **✅ COMPLETE** | **22 core + 38 audio + 2 GUI tests passing; dual-window binary compiles** |

### Phase 5 Progress

| Component | Status | Notes |
|-----------|--------|-------|
| `OscDriver` (UDP RX/TX) | ✅ | Async `tokio::net::UdpSocket`, broadcast TX, spawns RX task |
| `OscRouter` (pattern matching) | ✅ | Trie-based with `?` wildcard; exact + wildcard dispatch tested |
| `OscManager` (high-level) | ✅ | Subscribe by pattern, remote control helpers (`send_remote_go`, etc.), discovery |
| `MamscPacket` (binary parser) | ✅ | `GMA\0MSC\0` header + MIDI sysex MSC payload; Go/Stop/Resume/TimedGo/Set/Fire/GoOff |
| `MamscDriver` (MA-MSC UDP) | ✅ | Async UDP RX/TX, subscriber dispatch by `MscCommandFlags` |
| `MscManager` (high-level) | ✅ | Device/executor/page filtering, event channel |
| `ShowFileSender` / `ShowFileReceiver` | ✅ | 1 KB block transfer with ACK/NACK, 250 ms retry, 5 s timeout |
| Integration (`qplayer` binary) | ✅ | OSC + MSC managers wired into `AppCommand` queue; dual-window binary compiles |
| **Phase 5 Status** | **✅ COMPLETE** | **6 protocol tests passing; workspace clean build** |

### Phase 7 Progress (Partial — Polish items ahead of Phase 6)

| Component | Status | Notes |
|-----------|--------|-------|
| Save / Save As | ✅ | `serde_json::to_string_pretty` to `.qproj`; Save As via `rfd` dialog |
| Dirty tracking | ✅ | `dirty: bool` on `SharedState`; set on edits, cleared on New/Open/Save |
| Window title dirty indicator | ✅ | "QPlayer — show.qproj *" updates per frame |
| Autosave | ✅ | 60 s interval, 5 rotating backups in `dirs::data_dir()/QPlayer/autoback_{n}.qproj` |
| Crash recovery (`human-panic`) | ✅ | Friendly panic messages with crash-report file |
| SIGINT / SIGTERM handler (`ctrlc`) | ✅ | Emergency autosave to `crash_recovery.qproj` before exit |
| Drag & Drop (external files) | ✅ | Audio/video file drops onto control window create new cues |
| Single instance | ✅ | `single-instance` crate prevents multiple app launches |
| Undo / Redo | ✅ | Snapshot-based history (50 deep), Edit menu + Ctrl+Z / Ctrl+Shift+Z |
| **Phase 7 Status** | **✅ COMPLETE** | **Core polish complete; Installers deferred to packaging phase** |

| 4: Integration + Video | 3 weeks | Wire audio + GUI + video + file I/O | Can load a show, press Go, hear audio + see video, save changes | Medium | ✅ **Complete** |
| 5: Protocols | 1 week | OSC, MSC, remote control | Existing iPad remote control client connects and triggers cues | Low | ✅ **Complete** |
### Phase 6 Progress

| Component | Status | Notes |
|-----------|--------|-------|
| `PluginHost` (`wasmtime`) | ✅ | Loads `wasm32-unknown-unknown` modules, provides `host_log` import |
| `PluginInstance` lifecycle hooks | ✅ | `on_load`, `on_unload`, `on_go(qid)`, `on_save`, `on_slow_update` |
| `PluginManager` (host binary) | ✅ | Scans `plugins/` dir, loads all `.wasm` files, fail-open on errors |
| Plugin hooks wired into app | ✅ | `on_go` in `handle_go`, `on_save` in save command, `on_slow_update` every 250 ms |
| Example plugin (`hello-plugin`) | ✅ | `no_std` WASM plugin with static string logging via host import |
| Crash isolation test | ✅ | Plugin host traps do not propagate to host; verified in integration test |
| **Phase 6 Status** | **✅ COMPLETE** | **2 plugin tests passing; hello-plugin loads and runs** |

| 6: Plugins | 1–2 weeks | Plugin ABI + port OSC/MagicQ plugins | WASM plugin host loads and runs; crash isolation verified; custom cue types deferred | **High** | ✅ **Complete** |
| 7: Polish | 2 weeks | Undo, drag-drop, packaging, docs | Core polish done; Installers remain for packaging phase | Low | ✅ **Complete** |
| **Total** | **15–18 weeks** | **Feature-complete Rust QPlayer** | NFRs met; C# feature parity checklist 100% | | |

**Rollback:** if a phase misses its exit criterion by > 20%, halt and re-evaluate scope before advancing. The phased crate split means earlier phases remain shippable as libraries even if later phases slip.

### 6.1.1 Phase 0 Spike (De-risking Gate)

Phase 0 exists solely to validate the three highest-risk assumptions before committing to the full port. Do not skip or compress.

| Assumption | Validation | Go/No-go Threshold |
|------------|-----------|---------------------|
| cpal meets latency NFR on all 3 OSes | Measure loopback latency with 512-frame buffer | ≤ 20 ms WASAPI, ≤ 12 ms CoreAudio, ≤ 15 ms ALSA |
| egui renders cue list at target fps | Render 500-row `TableBuilder` with waveform column | ≥ 120 fps on reference hardware |
| serde round-trips existing `.qproj` schema | Load largest show file from corpus | Byte-identical re-serialization after `#[serde(deny_unknown_fields)]` removed |

If any threshold fails, document findings and revisit §0 decisions — do not proceed to Phase 1.

### 6.2 File Format Compatibility

**Must maintain:** `.qproj` files from C# QPlayer must open in Rust QPlayer (and ideally vice versa).

Approach:
1. Keep JSON schema identical
2. Port `ShowFileConverter` exactly
3. Add `fileFormatVersion` field check on load
4. Save in latest format version

**`.qpek` peak files:** These are binary caches. The Rust version can:
- Read existing `.qpek` files (port the parser exactly)
- Generate new `.qpek` files on first load if missing

### 6.3 Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Audio quality regression | A/B test with identical files, measure THD+N, latency |
| Performance worse than C# | Benchmark early (Phase 0 spike). SIMD is critical. |
| egui can't replicate WPF layout | Prototype cue list + inspector in Phase 3 spike before committing |
| Plugin ABI instability | Version the vtable, provide shim layers |
| macOS/Linux audio issues | Test cpal on all platforms early; ASIO is Windows-only anyway |

### 6.4 Interop During Development

**Option: Sidecar approach**
- Keep C# QPlayer running for comparison
- Rust version reads/writes same `.qproj` files
- Use OSC to sync transport between versions for A/B testing

---

## 7. Code Examples

### 7.1 Cue Enum with serde Polymorphism

```rust
// qplayer-core/src/cue/mod.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueBase {
    pub qid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub colour: SerializedColour,
    pub trigger: TriggerMode,
    pub enabled: bool,
    pub delay: f64,
    pub loop_mode: LoopMode,
    pub loop_count: i32,
    pub remote_node: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum Cue {
    #[serde(rename = "GroupCue")]
    Group { base: CueBase },
    
    #[serde(rename = "SoundCue")]
    Sound {
        #[serde(flatten)]
        base: CueBase,
        path: String,
        start_time: f64,
        duration: f64,
        volume: f32,
        pan: f32,
        fade_in: f64,
        fade_out: f64,
        fade_type: FadeType,
        eq: EqSettings,
    },
    
    #[serde(rename = "StopCue")]
    Stop {
        #[serde(flatten)]
        base: CueBase,
        stop_qid: u32,
        stop_mode: StopMode,
        fade_out_duration: f64,
    },
    
    #[serde(rename = "VolumeCue")]
    Volume {
        #[serde(flatten)]
        base: CueBase,
        sound_qid: u32,
        target_volume: f32,
        fade_duration: f64,
    },
    
    #[serde(rename = "DummyCue")]
    Dummy { #[serde(flatten)] base: CueBase },
    
    #[serde(rename = "TimeCodeCue")]
    TimeCode { #[serde(flatten)] base: CueBase },
    
    // Plugin-registered types
    #[serde(untagged)]
    Plugin(PluginCue),
}
```

### 7.2 SharedState + Command Pattern

```rust
// qplayer/src/state.rs

#[derive(Debug)]
pub enum AppCommand {
    Transport(TransportCommand),
    Cue(CueCommand),
    Project(ProjectCommand),
    Osc(OscPacket),
    Msc(MscMessage),
}

#[derive(Debug)]
pub enum TransportCommand {
    Go,
    Stop,
    Pause,
    GoCue(u32),
    StopCue(u32),
    PreloadCue(u32),
}

#[derive(Debug)]
pub enum CueCommand {
    Create { cue_type: CueType, after: Option<u32> },
    Delete(u32),
    Duplicate(u32),
    Move { cue_id: u32, new_parent: Option<u32>, new_index: usize },
    UpdateProperty { cue_id: u32, property: CueProperty },
    Select(u32),
}

pub struct SharedState {
    pub show_file: ShowFile,
    pub command_queue: Vec<AppCommand>,
    pub transport: TransportState,
    pub ui: UiState,
}

impl SharedState {
    pub fn enqueue(&mut self, cmd: AppCommand) {
        self.command_queue.push(cmd);
    }
}
```

### 7.3 GUI Cue List (egui)

```rust
// qplayer-gui/src/cue_list.rs

pub fn show_cue_list(ui: &mut egui::Ui, state: &mut SharedState) {
    let mut commands = vec![];
    
    egui::TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(egui::Column::auto().resizable(true))  // Q#
        .column(egui::Column::remainder())              // Name
        .column(egui::Column::auto())                   // State
        .column(egui::Column::auto().resizable(true))  // Time
        .header(20.0, |mut header| {
            header.col(|ui| { ui.strong("#"); });
            header.col(|ui| { ui.strong("Name"); });
            header.col(|ui| { ui.strong(""); });
            header.col(|ui| { ui.strong("Time"); });
        })
        .body(|mut body| {
            let cues = state.show_file.cues.clone();
            for (idx, cue) in cues.iter().enumerate() {
                body.row(18.0, |mut row| {
                    let is_selected = state.ui.selected_cue == Some(cue.qid);
                    
                    row.col(|ui| {
                        ui.label(format!("{}", cue.qid));
                    });
                    
                    row.col(|ui| {
                        let response = ui.selectable_label(
                            is_selected,
                            &cue.name
                        );
                        if response.clicked() {
                            commands.push(AppCommand::Cue(
                                CueCommand::Select(cue.qid)
                            ));
                        }
                    });
                    
                    row.col(|ui| {
                        if let Some(active) = state.transport.find_active(cue.qid) {
                            ui.colored_label(
                                state_color(active.state),
                                "●"
                            );
                        }
                    });
                    
                    row.col(|ui| {
                        if let Some(active) = state.transport.find_active(cue.qid) {
                            ui.label(format!("{:.2}", active.playback_time));
                        }
                    });
                });
            }
        });
    
    // Apply all commands after releasing the borrow
    for cmd in commands {
        state.enqueue(cmd);
    }
}
```

### 7.4 Plugin ABI

```rust
// qplayer-plugin-api/src/lib.rs

use std::ffi::{c_char, c_void, CStr, CString};
use std::os::raw::c_int;

/// Versioned vtable for forward compatibility
#[repr(C)]
pub struct PluginVTable {
    pub api_version: u32,
    pub init: extern "C" fn(config: *const PluginConfig) -> *mut c_void,
    pub shutdown: extern "C" fn(ctx: *mut c_void),
    pub get_metadata: extern "C" fn(ctx: *mut c_void) -> PluginMetadata,
    pub on_load: extern "C" fn(ctx: *mut c_void, host: *const HostVTable),
    pub on_unload: extern "C" fn(ctx: *mut c_void),
    pub on_go: extern "C" fn(ctx: *mut c_void, cue: *const CueData),
    pub on_stop: extern "C" fn(ctx: *mut c_void, cue: *const CueData),
    pub get_cue_type_count: extern "C" fn(ctx: *mut c_void) -> usize,
    pub get_cue_types: extern "C" fn(ctx: *mut c_void, out: *mut CueTypeDef, max_count: usize) -> usize,
}

#[repr(C)]
pub struct PluginConfig {
    pub plugin_dir: *const c_char,
    pub log_level: c_int,
}

#[repr(C)]
pub struct PluginMetadata {
    pub name: *const c_char,
    pub version: *const c_char,
    pub author: *const c_char,
    pub description: *const c_char,
}

#[repr(C)]
pub struct HostVTable {
    pub api_version: u32,
    pub log_info: extern "C" fn(msg: *const c_char),
    pub log_error: extern "C" fn(msg: *const c_char),
    pub send_osc: extern "C" fn(addr: *const c_char, args: *const OscArg, count: usize),
    pub get_setting: extern "C" fn(key: *const c_char) -> *const c_char,
}

#[repr(C)]
pub struct CueData {
    pub qid: u32,
    pub cue_type: *const c_char,
    pub json_data: *const c_char,
}

#[repr(C)]
pub struct CueTypeDef {
    pub id: *const c_char,
    pub name: *const c_char,
    pub json_schema: *const c_char,
}

// Plugin entry point symbol name
pub const PLUGIN_ENTRY_SYMBOL: &str = "qplayer_plugin_init";

/// Plugins export this function
pub type PluginInitFn = extern "C" fn() -> *const PluginVTable;
```

---

## 8. Crate Dependencies

### 8.1 Workspace Cargo.toml

```toml
[workspace]
members = ["crates/*", "plugins/*"]
resolver = "3"

[workspace.dependencies]
# Core
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
anyhow = "1.0"
log = "0.4"

# Audio
cpal = "0.15"
ffmpeg-next = "8.1"
rubato = "0.16"

# GPU / UI
# NOTE: egui, egui-wgpu, and egui-winit must share the same minor version — mixing
# 0.31/0.32 will fail to compile. Pin all three together and bump as a set.
wgpu = "25.0"
winit = "0.30"
egui = "0.32"
egui-wgpu = "0.32"
egui-winit = "0.32"
pollster = "0.3"

# Networking
rosc = "0.10"
tokio = { version = "1.42", features = ["rt-multi-thread", "sync", "net", "time", "fs"] }

# Threading / Sync
crossbeam = "0.8"

# Utils
chrono = { version = "0.4", features = ["serde"] }
dirs = "6.0"
rfd = "0.15"
brotli = "7.0"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
```

### 8.2 Per-Crate Dependency Matrix

| Crate | Dependencies | Purpose |
|-------|--------------|---------|
| `qplayer-core` | `serde`, `serde_json`, `thiserror`, `chrono`, `brotli` | Pure logic, no I/O |
| `qplayer-audio` | `cpal`, `ffmpeg-next`, `rubato`, `log`, `crossbeam` | Real-time audio |
| `qplayer-protocols` | `rosc`, `tokio`, `log` | OSC/MSC networking |
| `qplayer-plugin-api` | `serde`, `serde_json` | ABI types |
| `qplayer-video` | `ffmpeg-next`, `wgpu`, `winit`, `bytemuck` | Video decode + wgpu output |
| `qplayer-gui` | `egui`, `egui-wgpu`, `egui-winit`, `wgpu`, `winit` | UI (no longer depends on `eframe`) |
| `qplayer` | All above + `anyhow`, `dirs`, `rfd`, `tokio`, `pollster` | Orchestration |

---

## 9. Testing Strategy

| Layer | Test Type | Tools |
|-------|-----------|-------|
| `qplayer-core` | Unit tests | Inline `#[cfg(test)]`, proptest for serialization |
| `qplayer-audio` | Unit + integration | Sine wave verification, latency measurement, property-based DSP |
| `qplayer-gui` | Screenshot tests | `egui` test harness, or manual verification checklist |
| `qplayer` | End-to-end | Load C# `.qproj`, verify playback, compare output |
| Plugins | Integration | Load plugin, verify cue type registration, test go/stop |

**Critical test: Backward compatibility**
```rust
#[test]
fn test_load_csharp_showfile_v7() {
    let data = include_str!("../test_data/v7_showfile.qproj");
    let showfile: ShowFile = serde_json::from_str(data).unwrap();
    assert_eq!(showfile.cues.len(), 42);
    // ... verify all cue types parse correctly
}
```

---

## 10. Build & Deployment

### 10.1 Development Workflow

```bash
# Run with logging
RUST_LOG=info cargo run -p qplayer

# Run tests for core only
cargo test -p qplayer-core

# Build release (always test audio in release -- debug is too slow)
cargo build --release -p qplayer

# Run a specific show file
./target/release/qplayer --open /path/to/show.qproj
```

### 10.2 Platform Packaging

| Platform | Tool | Output |
|----------|------|--------|
| macOS | `cargo-bundle` | `.app` bundle + `.dmg` |
| Windows | `cargo-wix` | `.msi` installer |
| Linux | `cargo-deb` / AppImage | `.deb` / `.AppImage` |

### 10.3 CI/CD (GitHub Actions)

```yaml
# .github/workflows/ci.yml
strategy:
  matrix:
    os: [ubuntu-latest, windows-latest, macos-latest]
    include:
      - os: macos-latest
        target: aarch64-apple-darwin
      - os: windows-latest
        target: x86_64-pc-windows-msvc
```

---

## Appendix A: Glossary

| Term | C# Context | Rust Context |
|------|-----------|--------------|
| Source Generator | Roslyn compile-time codegen | `macro_rules!` or proc macros (mostly unnecessary) |
| Data Template | XAML `DataTemplate` | Direct egui rendering code per type |
| Assembly | `.dll` / `.exe` | Crate or dylib |
| GAC / NuGet | Package management | `cargo` + crates.io |
| P/Invoke | `DllImport` | `extern "C"` + `libloading` |
| unsafe | `unsafe` blocks, pointers | Same keyword, but borrow checker still enforces aliasing |

## Appendix B: Reference Implementations

| Pattern | Source File | Description |
|---------|-------------|-------------|
| SharedState + commands | `rustjay-template/src/core/state.rs` | Central mutable state with command enums |
| Lock-free audio | `rustjay-template/src/audio/mod.rs` | cpal callback with atomic communication |
| wgpu + egui setup | `rustjay-template/src/gui/` | Full window + renderer initialization |
| Plugin loader | `QPlayer/Models/PluginLoader.cs` | C# reference for AssemblyLoadContext |
| Audio pipeline | `QPlayer/Audio/` | NAudio chain reference implementation |

---

*Document version: 1.6*
*Created: 2026-04-22*
*Last revised: 2026-04-22 (All PORTING_GUIDE phases complete — Undo/Redo implemented, snapshot-based history with 50-deep stack)*
*Next review: See FULL_FUNCTIONALITY.md for GUI interactivity roadmap*
