# QPlayer Rust Port — Full Functionality Roadmap

This document tracks the remaining work to reach feature parity with the C# QPlayer application. The architectural phases (core, audio, video, protocols, plugins) are complete. This roadmap focuses on **GUI interactivity, cue manipulation, and production usability**.

> Last updated: 2026-04-22 (Context menus on whole row, AfterLast auto-trigger, audio device hot-swap, .qpek peak file caching, full project settings window with Audio/OSC/MSC sections)
> Status: Foundation complete → GUI interactivity in progress

---

## Comparison with C# Original

### Where Rust is AHEAD

| Feature | C# Status | Rust Status | Notes |
|---------|-----------|-------------|-------|
| **Video playback** | ❌ Entirely stubbed (`VideoFile.cs` placeholder) | ✅ Full FFmpeg decode + wgpu output, A/V sync | Rust port has working video; C# never implemented it |
| **Cross-platform** | ⚠️ Windows-only (WPF) | ✅ macOS/Windows/Linux (egui+wgpu) | Native multi-platform from day one |

---

## Phase A: Cue Manipulation (Critical)

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| A.1 | **Editable inspector** | Two-way bound editors for every cue field | ✅ Text inputs, sliders, combo boxes, drag values for all cue types | Minor: no `Knob` control (rotary dial), no `HiddenTextbox` inline-edit pattern |
| A.2 | **Add cue** | Right-click menu, `Ctrl+T`, toolbar button | ✅ Toolbar + context menu; Sound/Video/Stop/Volume/Group/Dummy/TimeCode | — |
| A.3 | **Delete cue** | `Delete` key, context menu | ✅ Delete key + context menu | — |
| A.4 | **Duplicate cue** | `Ctrl+D` | ✅ Ctrl+D + context menu | — |
| A.5 | **Move cue up/down** | `Ctrl+↑/↓`, drag-and-drop | ✅ `Ctrl+↑/↓` swaps cue position; context menu items too | — |
| A.6 | **QID auto-assignment** | `ChooseQID()` with decimal subdivision (1 → 1.1 → 1.01) | ✅ `ShowFile::choose_qid()` with 6-level decimal subdivision fallback to max+1 | — |
| A.7 | **Colour picker** | Full picker in inspector | ✅ `ui.color_edit_button_srgba` wired to `SerializedColour` in inspector | — |
| A.8 | **Cue list columns** | Q#, Playback, Name, Enabled, Trigger, Wait, Duration, Loop | ⚠️ Only Q#, Name, Type, Colour swatch | Missing: Playback progress bar, Enabled checkbox, Trigger, Wait, Duration, Loop columns |
| A.9 | **Inline editing in cue list** | `HiddenTextbox` / `HiddenComboBox` for direct row edit | ❌ Not implemented | C# allows editing QID, Name, Trigger, etc. directly in the list row |
| A.10 | **Parent / Group hierarchy** | `parent` field on `CueBase` for nested grouping | ✅ Data model has `parent: Option<Decimal>` | Not wired in UI (no visual tree/group expansion) |

### Exit Criterion
User can create, edit, delete, duplicate, and reorder cues entirely through the GUI without editing `.qproj` files by hand.

---

## Phase B: Transport & Playback (Critical)

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| B.1 | **Stop audio** | `AudioEngine.stop_all()` | ✅ `Mixer::stop_all()` clears all inputs; wired to `App::stop_all()` | — |
| B.2 | **Pause / Resume** | Pauses active cues, resumes from pause point | ✅ Toggles `MixerInput::active` on all active cues; toggle button in transport | — |
| B.3 | **Preload** | Decode to specific time, pause, ready to go | ❌ Not implemented | C# has preload button + time field in left panel |
| B.4 | **Cue state machine** | Ready → Delay → Playing → Paused → Done | ⚠️ Basic tracking via `ActiveCue` struct with `qid/name/input` and `paused` flag | Missing: `Ready`/`Delay`/`Playing`/`Done` states; no state transitions |
| B.5 | **Trigger modes** | Go / WithLast / AfterLast with delay | ✅ `WithLast` fires consecutive cues together; `AfterLast` auto-triggers chain when previous cue finishes naturally | — |
| B.6 | **Stop cue behavior** | Find target by QID, apply fade-out | ✅ Finds target in `active_cues`, thread-based fade-out over `fade_out_time`, then deactivates | — |
| B.7 | **Volume cue behavior** | Find target sound cue, apply volume fade | ✅ Finds target in `active_cues`, thread-based volume fade over `fade_time` to target dB | — |
| B.8 | **Active cues panel** | Left panel showing running cues with live status | ✅ Left `SidePanel` with QID, name, pause indicator, and tiny volume meter | Minor: no per-cue progress bar, no individual stop/go/pause buttons |
| B.9 | **Delay / Wait** | Per-cue `Delay` (`TimeSpan`) deferred start | ❌ Not implemented | Data model has `delay` field but runtime ignores it |
| B.10 | **Looping** | `LoopMode`: OneShot, Looped, LoopedInfinite, HoldLast | ❌ Not implemented | Data model has `loop_mode`/`loop_count` but runtime ignores it |
| B.11 | **Playback progress** | Per-cue progress bar + time display in list | ❌ Not implemented | C# shows progress bar and elapsed time in each cue row |

### Exit Criterion
Pressing Go, Stop, Pause behaves identically to C# for SoundCue, VideoCue, StopCue, and VolumeCue.

---

## Phase C: Input & Shortcuts (Important)

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| C.1 | **Keyboard shortcuts** | Space=Go, Esc=Stop, Del=Delete, Ctrl+N/O/S/T/D | ✅ Space=Go, Esc=Stop, Del=Delete, Ctrl+N=New, Ctrl+O=Open, Ctrl+S=Save, Ctrl+T=Add Sound, Ctrl+D=Duplicate, Ctrl+Z/Shift+Z=Undo/Redo, Ctrl+↑/↓=Move | Missing: `[`/`Backspace`=Pause, `]`=Unpause, `Up`/`Down`=navigate, `Ctrl+E`=Pack, `Ctrl+Shift+S`=Save As |
| C.2 | **Context menus** | Right-click on cue list (add/move/delete/duplicate) | ✅ Right-click anywhere on cue row shows menu: Move Up/Down, Duplicate, Delete, Add Sound/Video/Stop/Volume Cue | — |
| C.3 | **Drag-and-drop reordering** | Visual ghost, auto-scroll, Ctrl+drag to copy | ✅ Drag handle (≡) on each row, drop onto target cue to reorder | Missing: auto-scroll, ghost panel, Ctrl+drag to copy |
| C.4 | **Drag-and-drop .qproj** | Open show file by dropping onto window | ✅ Dropping `.qproj` queues `OpenProject` command with full guard support | — |
| C.5 | **Drag-and-drop media** | External audio/video files → new cues | ✅ Dropping audio/video creates new Sound/Video cues | — |

### Exit Criterion
Power user can operate the entire application without a mouse.

---

## Phase D: Undo / Redo (Important)

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| D.1 | **Undo stack** | 50-action history, snapshot-based | ✅ Complete — `UndoRedo` struct, push/undo/depth-cap, tests passing | — |
| D.2 | **Redo stack** | Inverse of undo | ✅ Complete — redo/pop/push-to-undo, tests passing | — |
| D.3 | **Keyboard shortcuts** | `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` | ✅ Complete — `Ctrl+Z` and `Ctrl+Shift+Z` bound; `Ctrl+Y` not bound but Shift+Z works | — |
| D.4 | **Unsaved-changes guard** | Prompt on New/Open/Exit if dirty | ✅ `rfd::MessageDialog` with OK/Cancel on window close, New, and Open | — |
| D.5 | **Running-cues guard** | Warn if cues are playing before exit/open/new | ✅ Checked before window close, New, and Open; prompts to stop all running cues | — |
| D.6 | **Action merging** | Consecutive changes to same property collapse into one undo | ❌ Not implemented | C# merges rapid slider adjustments into single undo |

### Exit Criterion
User can accidentally delete a cue, press Ctrl+Z, and recover it.

---

## Phase E: Audio & Visual Polish (Important)

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| E.1 | **Audio meters** | Peak/RMS/clip in GUI | ✅ Stereo peak/RMS meter bridge in transport bar (green/yellow/red segments) | Minor: no clip indicator LED |
| E.2 | **Waveform display** | Peak-file cached waveform per cue | ✅ Real-time peak generation (200 bars) from audio file, rendered as green bars in inspector | Minor: no zoom/pan, no draggable in/out markers, no pop-out window |
| E.3 | **EQ editor** | Per-cue parametric EQ GUI | ✅ Enable/disable toggle, HPF/LPF with order, 4 bands with shape/freq/gain/Q | Minor: C# has more band shapes (AllPass, Notch, HighShelf, LowShelf); no SIMD yet |
| E.4 | **Audio device selection** | WASAPI/ASIO/DirectSound picker | ✅ Dropdown in Project Settings lists all devices; selecting one recreates audio engine | — |
| E.5 | **Audio limiter settings** | Threshold/attack/release GUI | ✅ Master limiter threshold slider (-24 dB to 0 dB) in Project Settings | Major gap: C# has lookahead limiter with auto-gain, soft-clip, GR metering; Rust has simple brickwall clamp |
| E.6 | **Peak file generation** | Auto-generate `.qpek` on first load | ✅ Simple `.qpek` sidecar format (magic+version+count+(min,max) pairs); loaded if present, generated and saved on first decode | Gap: C# has multi-resolution pyramid with Brotli compression; Rust format is flat |
| E.7 | **Audio file reading** | Lock-free double-buffered reading | ⚠️ Direct FFmpeg decoder read in audio callback | Gap: C# has dedicated start buffer for seamless looping, intelligent seek reuse, worker pool pre-fill |
| E.8 | **Per-cue progress** | Playback position display | ❌ Not implemented | C# shows elapsed/total time and progress bar per active cue |

### Exit Criterion
Sound designer can see waveforms, set EQ, and monitor levels without leaving the app.

---

## Phase F: Settings & Configuration (Important)

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| F.1 | **Project settings panel** | OSC/MSC/remote node config | ✅ Full settings window with collapsible sections: Show Info, Audio (latency, exclusive mode, device picker, limiter), OSC/Remote (NIC, ports, enable, node name), MSC (enable, ports) | — |
| F.2 | **Recent files menu** | Persisted across sessions | ✅ Recent files saved to `~/.config/QPlayer/settings.json` and restored on launch | — |
| F.3 | **Settings window** | Persistent app preferences | ✅ Project settings are persisted in `.qproj` file; app-level settings (recent files) in `~/.config/QPlayer/settings.json` | Minor: C# has a separate read-only Shortcuts/OSCs reference window |
| F.4 | **Remote nodes window** | OSC remote node management | ❌ Not implemented | C# has full remote node editor with discovery timeout, host/client mode |
| F.5 | **Plugin manager window** | List loaded plugins | ❌ Not implemented | C# shows plugin list with metadata and registered cue types |
| F.6 | **Log window** | Live application log viewer | ❌ Not implemented | C# has non-modal log window with Clear/Save and auto-scroll |

### Exit Criterion
User can configure OSC ports, audio devices, and remote nodes without editing files.

---

## Phase G: Additional Windows & UI Polish

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| G.1 | **About window** | Version, copyright, credits | ❌ Not implemented | Simple modal dialog |
| G.2 | **Status bar** | Status text, Show Mode toggle, Audio Active indicator | ❌ Not implemented | C# has bottom status bar with audio activity LED |
| G.3 | **Progress overlay** | Modal blocking overlay for long operations | ❌ Not implemented | Used during showfile sync, packing |
| G.4 | **Waveform pop-out window** | Detachable expanded waveform view | ❌ Not implemented | C# has `WaveFormWindow` with transport controls |
| G.5 | **Themes** | Dark, Light, Red themes | ❌ Not implemented | C# has theme switching via `ThemeType` |
| G.6 | **Menu bar completeness** | File, Edit, Window, Help | ⚠️ File + Edit only | Missing: Window menu (Log, Remote Nodes, Plugins, Settings), Help menu (Manual, About) |

---

## Phase H: File Operations & Project Management

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| H.1 | **Pack Project** | Copy all media into self-contained `Media/` folder | ❌ Not implemented | Rewrites paths to relative |
| H.2 | **Path resolution** | Auto-resolve relative paths; search project tree if moved | ⚠️ Basic relative path handling | C# has intelligent path search fallback |
| H.3 | **Autosave toggle** | Menu checkbox to enable/disable autosave | ❌ Not implemented | C# can toggle autosave from File menu |
| H.4 | **Show file version** | Auto-upgrade V2→V3→V4→V7 | ✅ Migrations V2→V3→V4→V7 implemented | — |

---

## Phase I: Advanced Playback Features

| # | Feature | C# Reference | Rust Status | Gap |
|---|---------|--------------|-------------|-----|
| I.1 | **OSC Cue** | Plugin-registered cue type for sending OSC messages | ❌ Not implemented | C# has `OSCCueModel` with raw OSC address + args |
| I.2 | **TimeCode Cue runtime** | Execute at specific timecode | ❌ Not implemented | Data model exists but no runtime |
| I.3 | **Remote node per cue** | `remoteNode` field for remote-control target | ❌ Not implemented | Data model has field but runtime ignores it |
| I.4 | **MagicQCTRL hardware** | USB HID controller support | ❌ Not feasible | C# has full USB HID integration; Rust would need platform-specific HID |

---

## Summary: Biggest Gaps vs. C#

### Critical for parity (would block a power user):
1. **B.3 Preload** — decode to position, pause, ready to go
2. **B.9 Delay / Wait** — per-cue deferred start
3. **B.10 Looping** — OneShot/Looped/LoopedInfinite/HoldLast
4. **B.11 Playback progress** — per-cue progress bar + time display
5. **A.8 Cue list columns** — missing Playback, Enabled, Trigger, Wait, Duration, Loop
6. **A.9 Inline editing** — direct row edit without opening inspector

### Important for production use:
7. **E.5 Full limiter** — lookahead, auto-gain, gain reduction metering
8. **E.7 Double-buffered audio reading** — seamless looping, intelligent seek
9. **F.4 Remote nodes window** — OSC remote node management
10. **F.6 Log window** — live log viewer
11. **G.6 Complete menu bar** — Window and Help menus
12. **H.1 Pack Project** — self-contained media distribution

### Nice-to-have polish:
13. **E.2 Waveform zoom/pan** + draggable in/out markers
14. **D.6 Undo action merging** — collapse consecutive slider adjustments
15. **G.4 Waveform pop-out window**
16. **G.5 Theme switching**
17. **F.5 Plugin manager window**

### Where Rust is already superior:
- **Video playback** (C# never implemented)
- **Cross-platform support** (C# is Windows-only WPF)
- **WASM sandboxed plugins** (C# uses .dll with AssemblyLoadContext — less secure)
