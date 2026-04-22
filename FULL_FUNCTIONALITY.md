# QPlayer Rust Port — Full Functionality Roadmap

This document tracks the remaining work to reach feature parity with the C# QPlayer application. The architectural phases (core, audio, video, protocols, plugins) are complete. This roadmap focuses on **GUI interactivity, cue manipulation, and production usability**.

> Last updated: 2026-04-22
> Status: Foundation complete → GUI interactivity in progress

---

## Phase A: Cue Manipulation (Critical)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| A.1 | **Editable inspector** | Two-way bound editors for every cue field | ❌ Read-only labels | `qplayer-gui/src/inspector/mod.rs` |
| A.2 | **Add cue** | Right-click menu, `Ctrl+T`, toolbar button | ❌ Not implemented | `qplayer-gui/src/cue_list/mod.rs`, `app/mod.rs` |
| A.3 | **Delete cue** | `Delete` key, context menu | ❌ Not implemented | `qplayer-gui/src/cue_list/mod.rs`, `app/mod.rs` |
| A.4 | **Duplicate cue** | `Ctrl+D` | ❌ Not implemented | `qplayer-gui/src/app/mod.rs` |
| A.5 | **Move cue up/down** | `Ctrl+↑/↓`, drag-and-drop | ❌ Not implemented | `qplayer-gui/src/cue_list/mod.rs` |
| A.6 | **QID auto-assignment** | `ChooseQID()` with decimal subdivision (1 → 1.1 → 1.01) | ⚠️ Sequential integers only on drag-drop | `qplayer-core/src/cue/mod.rs` |
| A.7 | **Colour picker** | Full picker in inspector | ⚠️ Display-only swatch | `qplayer-gui/src/inspector/mod.rs` |

### Exit Criterion
User can create, edit, delete, duplicate, and reorder cues entirely through the GUI without editing `.qproj` files by hand.

---

## Phase B: Transport & Playback (Critical)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| B.1 | **Stop audio** | `AudioEngine.stop_all()` | ❌ Stub (`// TODO: stop audio playback`) | `qplayer/src/main.rs` |
| B.2 | **Pause / Resume** | Pauses active cues, resumes from pause point | ❌ Transport buttons queue commands but don't affect audio | `qplayer/src/main.rs`, `qplayer-audio/src/engine.rs` |
| B.3 | **Preload** | Decode to specific time, pause, ready to go | ❌ Not implemented | `qplayer/src/main.rs` |
| B.4 | **Cue state machine** | Ready → Delay → Playing → Paused → Done | ❌ No runtime state tracking | `qplayer-gui/src/app/mod.rs` |
| B.5 | **Trigger modes** | Go / Follow Last / After Last with delay | ❌ Go plays only selected cue | `qplayer/src/main.rs` |
| B.6 | **Stop cue behavior** | Find target by QID, apply fade-out | ❌ Data model only | `qplayer-audio/src/engine.rs` |
| B.7 | **Volume cue behavior** | Find target sound cue, apply volume fade | ❌ Data model only | `qplayer-audio/src/engine.rs` |
| B.8 | **Active cues panel** | Left panel showing running cues with live status | ❌ Not implemented | `qplayer-gui/src/app/mod.rs` |

### Exit Criterion
Pressing Go, Stop, Pause behaves identically to C# for SoundCue, VideoCue, StopCue, and VolumeCue.

---

## Phase C: Input & Shortcuts (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| C.1 | **Keyboard shortcuts** | Space=Go, Esc=Stop, Del=Delete, Ctrl+N/O/S/T/D | ❌ None bound | `qplayer/src/main.rs` |
| C.2 | **Context menus** | Right-click on cue list (add/move/delete/duplicate) | ❌ Not implemented | `qplayer-gui/src/cue_list/mod.rs` |
| C.3 | **Drag-and-drop reordering** | Visual ghost, auto-scroll, Ctrl+drag to copy | ❌ Not implemented | `qplayer-gui/src/cue_list/mod.rs` |
| C.4 | **Drag-and-drop .qproj** | Open show file by dropping onto window | ❌ Audio/video only | `qplayer/src/main.rs` |

### Exit Criterion
Power user can operate the entire application without a mouse.

---

## Phase D: Undo / Redo (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| D.1 | **Undo stack** | 50-action history, snapshot-based | 🏗️ In progress | `qplayer-gui/src/app/mod.rs` |
| D.2 | **Redo stack** | Inverse of undo | 🏗️ In progress | `qplayer-gui/src/app/mod.rs` |
| D.3 | **Keyboard shortcuts** | `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` | 🏗️ In progress | `qplayer/src/main.rs` |
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

1. **Editable inspector** — unlocks all cue property editing
2. **Add / delete / duplicate cues** — unlocks cue list manipulation
3. **Keyboard shortcuts** — unlocks power-user workflows
4. **Undo / Redo** — unlocks safe experimentation
5. **Transport that actually works** — unlocks reliable playback

These five items transform the Rust port from a "read-only viewer" into a "usable cue list editor."

---

## How to Use This Document

- Check off items as they are implemented
- Update the Status column and Last Updated date
- Link to specific PRs or commits in the Notes column
- When a phase's Exit Criterion is met, mark the phase ✅ Complete
