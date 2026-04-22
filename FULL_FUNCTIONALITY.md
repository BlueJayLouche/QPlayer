# QPlayer Rust Port — Full Functionality Roadmap

This document tracks the remaining work to reach feature parity with the C# QPlayer application. The architectural phases (core, audio, video, protocols, plugins) are complete. This roadmap focuses on **GUI interactivity, cue manipulation, and production usability**.

> Last updated: 2026-04-22 (Context menus on whole row, AfterLast auto-trigger, audio device hot-swap, .qpek peak file caching, full project settings window with Audio/OSC/MSC sections)
> Status: Foundation complete → GUI interactivity in progress

---

## Phase A: Cue Manipulation (Critical)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| A.1 | **Editable inspector** | Two-way bound editors for every cue field | ✅ Text inputs, sliders, combo boxes, drag values for all cue types | `qplayer-gui/src/inspector/mod.rs` |
| A.2 | **Add cue** | Right-click menu, `Ctrl+T`, toolbar button | ✅ Toolbar + context menu; Sound/Video/Stop/Volume/Group/Dummy/TimeCode | `qplayer-gui/src/cue_list/mod.rs`, `app/mod.rs` |
| A.3 | **Delete cue** | `Delete` key, context menu | ✅ Delete key + context menu | `qplayer-gui/src/cue_list/mod.rs`, `app/mod.rs` |
| A.4 | **Duplicate cue** | `Ctrl+D` | ✅ Ctrl+D + context menu | `qplayer-gui/src/app/mod.rs` |
| A.5 | **Move cue up/down** | `Ctrl+↑/↓`, drag-and-drop | ✅ `Ctrl+↑/↓` swaps cue position in list; context menu items too | `qplayer-gui/src/cue_list/mod.rs`, `app/mod.rs` |
| A.6 | **QID auto-assignment** | `ChooseQID()` with decimal subdivision (1 → 1.1 → 1.01) | ✅ `ShowFile::choose_qid()` with 6-level decimal subdivision fallback to max+1 | `qplayer-core/src/showfile/mod.rs` |
| A.7 | **Colour picker** | Full picker in inspector | ✅ `ui.color_edit_button_srgba` wired to `SerializedColour` in inspector | `qplayer-gui/src/inspector/mod.rs` |

### Exit Criterion
User can create, edit, delete, duplicate, and reorder cues entirely through the GUI without editing `.qproj` files by hand.

---

## Phase B: Transport & Playback (Critical)

> ~~**Blocker: command queue double-drain bug.**~~ **FIXED**
> `QPlayerApp::process_commands` no longer consumes `Go`/`Stop`/`Pause`; they pass through to `main.rs::process_commands`. Transport buttons now trigger actual playback and stop.

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| B.1 | **Stop audio** | `AudioEngine.stop_all()` | ✅ `Mixer::stop_all()` clears all inputs; wired to `App::stop_all()` | `qplayer/src/main.rs:374` |
| B.2 | **Pause / Resume** | Pauses active cues, resumes from pause point | ✅ Toggles `MixerInput::active` on all active cues; toggle button in transport | `qplayer/src/main.rs` |
| B.3 | **Preload** | Decode to specific time, pause, ready to go | ❌ Not implemented | `qplayer/src/main.rs` |
| B.4 | **Cue state machine** | Ready → Delay → Playing → Paused → Done | ⚠️ Basic tracking via `ActiveCue` struct with `qid/name/input` and `paused` flag | `qplayer/src/main.rs` |
| B.5 | **Trigger modes** | Go / Follow Last / After Last with delay | ✅ `WithLast` fires consecutive cues together; `AfterLast` auto-triggers chain when previous cue finishes naturally (non-audio AfterLasts fire in a burst, first audio AfterLast waits for its own finish) | `qplayer-gui/src/inspector/mod.rs`, `qplayer/src/main.rs` |
| B.6 | **Stop cue behavior** | Find target by QID, apply fade-out | ✅ Finds target in `active_cues`, thread-based fade-out over `fade_out_time`, then deactivates | `qplayer/src/main.rs` |
| B.7 | **Volume cue behavior** | Find target sound cue, apply volume fade | ✅ Finds target in `active_cues`, thread-based volume fade over `fade_time` to target dB | `qplayer/src/main.rs` |
| B.8 | **Active cues panel** | Left panel showing running cues with live status | ✅ Left `SidePanel` with QID, name, pause indicator, and tiny volume meter (green/yellow/red) | `qplayer-gui/src/active_cues/mod.rs` |

### Exit Criterion
Pressing Go, Stop, Pause behaves identically to C# for SoundCue, VideoCue, StopCue, and VolumeCue.

---

## Phase C: Input & Shortcuts (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| C.1 | **Keyboard shortcuts** | Space=Go, Esc=Stop, Del=Delete, Ctrl+N/O/S/T/D | ✅ Space=Go, Esc=Stop, Del=Delete, Ctrl+N=New, Ctrl+O=Open, Ctrl+S=Save, Ctrl+T=Add Sound, Ctrl+D=Duplicate, Ctrl+Z/Shift+Z=Undo/Redo, Ctrl+↑/↓=Move | `qplayer-gui/src/app/mod.rs` |
| C.2 | **Context menus** | Right-click on cue list (add/move/delete/duplicate) | ✅ Right-click anywhere on cue row shows menu: Move Up/Down, Duplicate, Delete, Add Sound/Video/Stop/Volume Cue | `qplayer-gui/src/cue_list/mod.rs` |
| C.3 | **Drag-and-drop reordering** | Visual ghost, auto-scroll, Ctrl+drag to copy | ✅ Drag handle (≡) on each row, drop onto target cue to reorder | `qplayer-gui/src/cue_list/mod.rs` |
| C.4 | **Drag-and-drop .qproj** | Open show file by dropping onto window | ✅ Dropping `.qproj` queues `OpenProject` command with full guard support | `qplayer/src/main.rs` |

### Exit Criterion
Power user can operate the entire application without a mouse.

---

## Phase D: Undo / Redo (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| D.1 | **Undo stack** | 50-action history, snapshot-based | ✅ Complete — `UndoRedo` struct, push/undo/depth-cap, tests passing | `qplayer-gui/src/app/mod.rs` |
| D.2 | **Redo stack** | Inverse of undo | ✅ Complete — redo/pop/push-to-undo, tests passing | `qplayer-gui/src/app/mod.rs` |
| D.3 | **Keyboard shortcuts** | `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` | ✅ Complete — `Ctrl+Z` and `Ctrl+Shift+Z` bound (`app/mod.rs:207`); `Ctrl+Y` not bound but Shift+Z works | `qplayer-gui/src/app/mod.rs` |
| D.4 | **Unsaved-changes guard** | Prompt on New/Open/Exit if dirty | ✅ `rfd::MessageDialog` with OK/Cancel on window close, New, and Open | `qplayer/src/main.rs`, `qplayer-gui/src/app/mod.rs` |
| D.5 | **Running-cues guard** | Warn if cues are playing before exit/open/new | ✅ Checked before window close, New, and Open; prompts to stop all running cues | `qplayer/src/main.rs`, `qplayer-gui/src/app/mod.rs` |

### Exit Criterion
User can accidentally delete a cue, press Ctrl+Z, and recover it.

---

## Phase E: Audio & Visual Polish (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| E.1 | **Audio meters** | Peak/RMS/clip in GUI | ✅ Stereo peak/RMS meter bridge in transport bar (green/yellow/red segments) | `qplayer-gui/src/transport/mod.rs` |
| E.2 | **Waveform display** | Peak-file cached waveform per cue | ✅ Real-time peak generation (200 bars) from audio file, rendered as green bars in inspector | `qplayer-gui/src/waveform/mod.rs` |
| E.3 | **EQ editor** | Per-cue parametric EQ GUI | ✅ Enable/disable toggle, HPF/LPF with order, 4 bands with shape/freq/gain/Q | `qplayer-gui/src/inspector/mod.rs` |
| E.4 | **Audio device selection** | WASAPI/ASIO/DirectSound picker | ✅ Dropdown in Project Settings lists all devices; selecting one recreates audio engine (stops all cues, fallback to default on error) | `qplayer/src/main.rs`, `qplayer-gui/src/app/mod.rs` |
| E.5 | **Audio limiter settings** | Threshold/attack/release GUI | ✅ Master limiter threshold slider (-24 dB to 0 dB) in Project Settings | `qplayer-gui/src/app/mod.rs`, `qplayer/src/main.rs` |
| E.6 | **Peak file generation** | Auto-generate `.qpek` on first load | ✅ Simple `.qpek` sidecar format (magic+version+count+(min,max) pairs); loaded if present, generated and saved on first decode | `qplayer-gui/src/waveform/mod.rs` |

### Exit Criterion
Sound designer can see waveforms, set EQ, and monitor levels without leaving the app.

---

## Phase F: Settings & Configuration (Important)

| # | Feature | C# Reference | Rust Status | Target File(s) |
|---|---------|--------------|-------------|----------------|
| F.1 | **Project settings panel** | OSC/MSC/remote node config | ✅ Full settings window with collapsible sections: Show Info, Audio (latency, exclusive mode, device picker, limiter), OSC/Remote (NIC, ports, enable, node name), MSC (enable, ports) | `qplayer-gui/src/app/mod.rs` |
| F.2 | **Recent files menu** | Persisted across sessions | ✅ Recent files saved to `~/.config/QPlayer/settings.json` and restored on launch | `qplayer-gui/src/app/mod.rs`, `qplayer/src/main.rs` |
| F.3 | **Settings window** | Persistent app preferences | ✅ Project settings are persisted in `.qproj` file; app-level settings (recent files) in `~/.config/QPlayer/settings.json` | `qplayer-gui/src/app/mod.rs` |
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
