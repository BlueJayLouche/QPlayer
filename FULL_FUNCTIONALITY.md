# QPlayer Rust Port — Full Functionality Roadmap

This document tracks the remaining work to reach feature parity with the C# QPlayer application. The architectural phases (core, audio, video, protocols, plugins) are complete. This roadmap focuses on **GUI interactivity, cue manipulation, and production usability**.

> Last updated: 2026-04-22 (editable inspector, add/delete/duplicate cues, keyboard shortcuts, command-queue fix, audio stop wired)
> Status: Foundation complete → GUI interactivity in progress

---

## Phase A: Cue Manipulation (Critical)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| A.1 | **Editable inspector** | Two-way bound editors for every cue field | ✅ Text inputs, sliders, combo boxes, drag values for all cue types | `qplayer-gui/src/inspector/mod.rs` |
| A.2 | **Add cue** | Right-click menu, `Ctrl+T`, toolbar button | ✅ Toolbar + context menu; Sound/Video/Stop/Volume/Group/Dummy/TimeCode | `qplayer-gui/src/cue_list/mod.rs`, `app/mod.rs` |
| A.3 | **Delete cue** | `Delete` key, context menu | ✅ Delete key + context menu | `qplayer-gui/src/cue_list/mod.rs`, `app/mod.rs` |
| A.4 | **Duplicate cue** | `Ctrl+D` | ✅ Ctrl+D + context menu | `qplayer-gui/src/app/mod.rs` |
| A.5 | **Move cue up/down** | `Ctrl+↑/↓`, drag-and-drop | ❌ Not implemented | `qplayer-gui/src/cue_list/mod.rs` |
| A.6 | **QID auto-assignment** | `ChooseQID()` with decimal subdivision (1 → 1.1 → 1.01) | ⚠️ Sequential integers only on drag-drop | `qplayer-core/src/cue/mod.rs` |
| A.7 | **Colour picker** | Full picker in inspector | ⚠️ Display-only swatch | `qplayer-gui/src/inspector/mod.rs` |

### Exit Criterion
User can create, edit, delete, duplicate, and reorder cues entirely through the GUI without editing `.qproj` files by hand.

---

## Phase B: Transport & Playback (Critical)

> ~~**Blocker: command queue double-drain bug.**~~ **FIXED**
> `QPlayerApp::process_commands` no longer consumes `Go`/`Stop`/`Pause`; they pass through to `main.rs::process_commands`. Transport buttons now trigger actual playback and stop.

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| B.1 | **Stop audio** | `AudioEngine.stop_all()` | ✅ `Mixer::stop_all()` clears all inputs; wired to `App::stop_all()` | `qplayer/src/main.rs:374` |
| B.2 | **Pause / Resume** | Pauses active cues, resumes from pause point | ⚠️ Commands reach main.rs now; Pause implementation still needed in audio engine | `qplayer/src/main.rs`, `qplayer-audio/src/engine.rs` |
| B.3 | **Preload** | Decode to specific time, pause, ready to go | ❌ Not implemented | `qplayer/src/main.rs` |
| B.4 | **Cue state machine** | Ready → Delay → Playing → Paused → Done | ❌ No runtime state tracking | `qplayer-gui/src/app/mod.rs` |
| B.5 | **Trigger modes** | Go / Follow Last / After Last with delay | ❌ Go plays only selected cue | `qplayer/src/main.rs` |
| B.6 | **Stop cue behavior** | Find target by QID, apply fade-out | ❌ Data model only. Also: `build_cue_chain` in `engine.rs:194` has TODO to wire EqProcessor, FadeProcessor, PanProcessor | `qplayer-audio/src/engine.rs` |
| B.7 | **Volume cue behavior** | Find target sound cue, apply volume fade | ❌ Data model only | `qplayer-audio/src/engine.rs` |
| B.8 | **Active cues panel** | Left panel showing running cues with live status | ❌ Not implemented | `qplayer-gui/src/app/mod.rs` |

### Exit Criterion
Pressing Go, Stop, Pause behaves identically to C# for SoundCue, VideoCue, StopCue, and VolumeCue.

---

## Phase C: Input & Shortcuts (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| C.1 | **Keyboard shortcuts** | Space=Go, Esc=Stop, Del=Delete, Ctrl+N/O/S/T/D | ✅ Space=Go, Esc=Stop, Del=Delete, Ctrl+T=Add Sound, Ctrl+D=Duplicate, Ctrl+Z/Shift+Z=Undo/Redo | `qplayer-gui/src/app/mod.rs` |
| C.2 | **Context menus** | Right-click on cue list (add/move/delete/duplicate) | ❌ Not implemented | `qplayer-gui/src/cue_list/mod.rs` |
| C.3 | **Drag-and-drop reordering** | Visual ghost, auto-scroll, Ctrl+drag to copy | ❌ Not implemented | `qplayer-gui/src/cue_list/mod.rs` |
| C.4 | **Drag-and-drop .qproj** | Open show file by dropping onto window | ❌ Audio/video only | `qplayer/src/main.rs` |

### Exit Criterion
Power user can operate the entire application without a mouse.

---

## Phase D: Undo / Redo (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| D.1 | **Undo stack** | 50-action history, snapshot-based | ✅ Complete — `UndoRedo` struct, push/undo/depth-cap, tests passing | `qplayer-gui/src/app/mod.rs` |
| D.2 | **Redo stack** | Inverse of undo | ✅ Complete — redo/pop/push-to-undo, tests passing | `qplayer-gui/src/app/mod.rs` |
| D.3 | **Keyboard shortcuts** | `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` | ✅ Complete — `Ctrl+Z` and `Ctrl+Shift+Z` bound (`app/mod.rs:207`); `Ctrl+Y` not bound but Shift+Z works | `qplayer-gui/src/app/mod.rs` |
| D.4 | **Unsaved-changes guard** | Prompt on New/Open/Exit if dirty | ❌ Not implemented | `qplayer/src/main.rs` |
| D.5 | **Running-cues guard** | Warn if cues are playing before exit/open/new | ❌ Not implemented | `qplayer/src/main.rs` |

### Exit Criterion
User can accidentally delete a cue, press Ctrl+Z, and recover it.

---

## Phase E: Audio & Visual Polish (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| E.1 | **Audio meters** | Peak/RMS/clip in GUI | ❌ Engine meters exist, no GUI | `qplayer-gui/src/app/mod.rs` |
| E.2 | **Waveform display** | Peak-file cached waveform per cue | ⚠️ `ui.label("Waveform (TODO)")` | `qplayer-gui/src/waveform/mod.rs` |
| E.3 | **EQ editor** | Per-cue parametric EQ GUI | ❌ Data model only | `qplayer-gui/src/inspector/mod.rs` |
| E.4 | **Audio device selection** | WASAPI/ASIO/DirectSound picker | ❌ `new_default()` only | `qplayer/src/main.rs` |
| E.5 | **Audio limiter settings** | Threshold/attack/release GUI | ❌ Data model only | `qplayer-gui/src/app/mod.rs` |
| E.6 | **Peak file generation** | Auto-generate `.qpek` on first load | ⚠️ `peakfile` module exists, not wired | `qplayer-audio/src/decoder.rs` |

### Exit Criterion
Sound designer can see waveforms, set EQ, and monitor levels without leaving the app.

---

## Phase F: Settings & Configuration (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| F.1 | **Project settings panel** | OSC/MSC/remote node config | ❌ Not implemented | `qplayer-gui/src/app/mod.rs` |
| F.2 | **Recent files menu** | Persisted across sessions | ❌ Not implemented | `qplayer-gui/src/app/mod.rs` |
| F.3 | **Settings window** | Persistent app preferences | ❌ Not implemented | `qplayer-gui/src/app/mod.rs` |
| F.4 | **Remote nodes window** | OSC remote node management | ❌ Not implemented | `qplayer-gui/src/app/mod.rs` |
| F.5 | **Plugin manager window** | List loaded plugins | ⚠️ Loads plugins, no UI | `qplayer-gui/src/app/mod.rs` |

### Exit Criterion
User can configure OSC ports, audio devices, and remote nodes without editing files.

---

## Phase G: Packaging (Nice-to-Have)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| G.1 | **Pack project** | Copy all referenced media into self-contained directory | ❌ Not implemented | New module |
| G.2 | **macOS .app bundle** | `cargo-bundle` | ❌ Not implemented | `Cargo.toml` |
| G.3 | **Windows .msi installer** | WiX via `cargo-wix` | ❌ Not implemented | New files |
| G.4 | **Linux AppImage/DEB** | `cargo-deb` or `cargo-appimage` | ❌ Not implemented | New files |
| G.5 | **Auto-update** | `self_update` crate or custom | ❌ Not implemented | New module |

### Exit Criterion
Double-click installer on all 3 OSes produces a working application.

---

## Immediate Next Steps

1. ~~**Fix command queue double-drain**~~ ✅
2. ~~**Wire audio stop**~~ ✅
3. ~~**Editable inspector**~~ ✅
4. ~~**Add / delete / duplicate cues**~~ ✅
5. ~~**Keyboard shortcuts**~~ ✅
6. **Move cue up/down** — unlocks cue ordering
7. **Cue state machine** — Ready → Delay → Playing → Paused → Done
8. **Transport: Pause / Preload / Stop cue behavior / Volume cue behavior**
9. **Active cues panel + audio meters**
10. **Waveform display**
11. **Project settings / audio device selection / recent files**

Items 1–2 transform the app from "plays audio on file-drop only" into "transport buttons actually work." Items 3–5 complete the editing foundation.

---

## How to Use This Document

- Check off items as they are implemented
- Update the Status column and Last Updated date
- Link to specific PRs or commits in the Notes column
- When a phase's Exit Criterion is met, mark the phase ✅ Complete
