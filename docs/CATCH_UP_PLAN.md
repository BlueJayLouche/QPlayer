# QPlayer Rust Port — Catch-Up Plan vs. C# Original

This document tracks every feature gap between the Rust port and the C# QPlayer original, organized by priority and estimated effort.

> **Current status:** 73 tests passing. **All P1 items complete!** Moving to P2.

---

## Priority Legend

| Priority | Meaning |
|----------|---------|
| **P0** | App is broken or unusable without this |
| **P1** | Power user would hit this daily; major parity gap |
| **P2** | Nice to have; occasional use or polish |
| **P3** | Would be cool; not blocking for most users |

---

## P0 — Critical Bugs (App Broken)

| # | Issue | Root Cause | Fix | Effort |
|---|-------|------------|-----|--------|
| P0.1 | ~~App locks up on exit~~ | Autosave thread sleeps 60s; process waits for it | ✅ Sleep 1s intervals, check flag | Done |
| P0.2 | ~~Go button doesn't playback cues~~ | `QPlayerApp::process_commands()` drains entire queue, drops `Go` in `_ => {}` | ✅ Collect unhandled, put back in queue | Done |
| P0.3 | ~~Drag-and-drop reordering broken~~ | `dnd_set_drag_payload`/`dnd_release_payload` on same widget in ScrollArea | ✅ Use `dnd_drag_source` + `dnd_drop_zone` properly | Done |

---

## P1 — Major Parity Gaps (Power Users Need These)

### Playback Runtime

| # | Feature | C# Behavior | Rust Gap | Target Files | Effort |
|---|---------|-------------|----------|--------------|--------|
| P1.1 | **Delay / Wait** | Per-cue `delay: TimeSpan` defers start by timer | ✅ Delay editor in inspector; `DelayedCue` queue checked each frame | `qplayer/src/main.rs`, `qplayer-gui/src/inspector/mod.rs` | Small |
| P1.2 | **Looping** | `LoopMode`: OneShot/Looped/LoopedInfinite/HoldLast with `loopCount` | ✅ `LoopProcessor` wired into `play_audio()`; start_time/duration trim points supported | `qplayer-audio/src/decoder.rs`, `qplayer/src/main.rs` | Medium |
| P1.3 | **Preload** | Decode to specific time, pause, ready to go on next Go | ✅ `handle_preload()` seeks + pauses; Go activates preloaded cue; transport has Preload button | `qplayer-gui/src/transport/mod.rs`, `qplayer/src/main.rs` | Done |
| P1.4 | **Playback progress** | Per-cue progress bar + elapsed/total time in cue list row | ✅ `MixerInput::position()/length()` synced to GUI; progress bars in active cues + cue list | `qplayer/src/main.rs`, `qplayer-gui/src/cue_list/mod.rs` | Small |
| P1.5 | **Cue state machine** | Ready → Delay → Playing/PlayingLooped ↔ Paused → Done | ✅ `CueState` enum with full transitions; state icons in active cues panel | `qplayer/src/main.rs` | Done |

### Cue List UX

| # | Feature | C# Behavior | Rust Gap | Target Files | Effort |
|---|---------|-------------|----------|--------------|--------|
| P1.6 | **More cue list columns** | Playback, Enabled, Trigger, Wait, Duration, Loop Mode | ✅ Trigger, Duration, Loop Mode added; progress bar for active cues | `qplayer-gui/src/cue_list/mod.rs` | Small |
| P1.7 | **Inline editing** | Edit QID/Name/Trigger directly in row (HiddenTextbox/HiddenComboBox) | ✅ Inline `TextEdit` for QID/Name, `ComboBox` for Trigger in Edit mode; undo snapshots captured per edit | `qplayer-gui/src/cue_list/mod.rs` | Done |

### Audio Engine Depth

| # | Feature | C# Behavior | Rust Gap | Target Files | Effort |
|---|---------|-------------|----------|--------------|--------|
| P1.8 | **Full master limiter** | Lookahead, soft-clip, auto-gain, gain-reduction metering | ✅ `Limiter` core with 5ms lookahead, attack/release envelope, stereo linking; wired into engine callback; GR meter in transport (0 to -30 dB) | `qplayer-audio/src/limiter_processor.rs`, `qplayer-audio/src/engine.rs` | Done |
| P1.9 | **Double-buffered file reading** | Lock-free double-buffered, intelligent seek reuse, start buffer for seamless looping | ✅ `BufferedSource` with 3-second ring buffer, background fill thread, lock-free atomic read/write positions, seek support | `qplayer-audio/src/buffered_source.rs` | Done |
| P1.10 | **Fade processor wiring** | `FadeProcessor` in audio chain (volume + pan fade) | ✅ Fade-in at play time via `FadeProcessor`; fade-out on stop + volume fades via `MixerInput::start_fade()` (per-frame, audio-thread) | `qplayer-audio/src/mixer.rs`, `qplayer/src/main.rs` | Done |

### Settings & Windows

| # | Feature | C# Behavior | Rust Gap | Target Files | Effort |
|---|---------|-------------|----------|--------------|--------|
| P1.11 | **Remote nodes window** | Full editor: discovery timeout, host/client mode, sync on save | ✅ Discovery heartbeat, RemoteDiscovery handling, node liveness (5s timeout), remote cue delegation, incoming remote command routing, inspector remote_node field, project settings UI | `qplayer/src/main.rs`, `qplayer-gui/src/app/mod.rs`, `qplayer-gui/src/inspector/mod.rs` | Done |
| P1.12 | **Log window** | Live log viewer with Clear/Save, auto-scroll, audio buffer debug | ✅ Custom `log::Log` implementation forwarding to `env_logger` + in-app ring buffer (2000 entries); colored level + timestamp + message; auto-scroll + Clear | `qplayer-gui/src/log_window/mod.rs`, `qplayer-gui/src/logging.rs` | Done |
| P1.13 | **Complete menu bar** | File (Pack, Autosave toggle), Edit, **Window**, **Help** | ✅ File, Edit, Window (Log toggle), Help (About) menus; About window with version + license | `qplayer-gui/src/app/mod.rs` | Done |

---

## P2 — Important for Production Use

| # | Feature | C# Behavior | Rust Gap | Target Files | Effort |
|---|---------|-------------|----------|--------------|--------|
| P2.1 | **Pack Project** | Copy all media into `Media/` folder, rewrite paths to relative | Not implemented | New module | Medium |
| P2.2 | **Path resolution** | Auto-resolve relative paths; search project tree if file moved | Basic relative path handling only | `qplayer-core/src/showfile/mod.rs` | Small |
| P2.3 | **Undo action merging** | Consecutive changes to same property collapse into one undo | Every change is a separate snapshot | `qplayer-gui/src/app/mod.rs` | Medium |
| P2.4 | **Autosave toggle** | Menu checkbox to enable/disable autosave | Always on; no toggle | `qplayer-gui/src/app/mod.rs` | Small |
| P2.5 | **EQ band shapes** | AllPass, Notch, HighShelf, LowShelf in addition to Bell/LowPass/HighPass | Missing 4 shapes | `qplayer-core/src/eq.rs`, `qplayer-gui/src/inspector/mod.rs` | Small |
| P2.6 | **Waveform zoom/pan** | Interactive zoom/pan navbar, draggable in/out markers | Static 200-bar view | `qplayer-gui/src/waveform/mod.rs` | Medium |
| P2.7 | **Waveform pop-out window** | Detachable expanded waveform with transport controls | Not implemented | `qplayer-gui/src/` (new module) | Medium |
| P2.8 | **Plugin manager window** | List loaded plugins, metadata, registered cue types | Not implemented | `qplayer-gui/src/` (new module) | Small |
| P2.9 | **Status bar** | Status text, Show Mode toggle, Audio Active/Inactive indicator | No status bar | `qplayer-gui/src/app/mod.rs` | Small |
| P2.10 | **Progress overlay** | Modal blocking overlay for long operations | Not implemented | `qplayer-gui/src/app/mod.rs` | Small |
| P2.11 | **OSC Cue** | Plugin-registered cue type for sending OSC messages | Not implemented | Plugin ABI extension | Medium |
| P2.12 | **TimeCode Cue runtime** | Execute at specific timecode | Data model exists; no runtime | `qplayer/src/main.rs` | Small |

---

## P3 — Nice-to-Have Polish

| # | Feature | C# Behavior | Rust Gap | Effort |
|---|---------|-------------|----------|--------|
| P3.1 | **Theme switching** | Dark, Light, Red themes via `ThemeType` | No theme support | Medium |
| P3.2 | **Knob control** | Rotary dial for dB/Hz values (mouse drag, double-click to type) | Using sliders and drag values instead | Medium |
| P3.3 | **About window** | Version, copyright, credits modal | Not implemented | Small |
| P3.4 | **Peak file pyramid** | Multi-resolution pyramid with Brotli compression | Flat `.qpek` format | Medium |
| P3.5 | **MagicQCTRL hardware** | USB HID controller support | Not feasible in pure Rust without platform-specific HID | Large |
| P3.6 | **Ctrl+E Pack shortcut** | Keyboard shortcut for Pack Project | Not bound | Tiny |
| P3.7 | **`[`/`]` Pause/Unpause** | Additional transport shortcuts | Not bound | Tiny |
| P3.8 | **Up/Down navigation** | Arrow keys move selection without Ctrl | Not bound | Tiny |

---

## Where Rust is Already Superior

| Feature | C# | Rust |
|---------|-----|------|
| Video playback | ❌ Entirely stubbed | ✅ Full implementation |
| Cross-platform | ⚠️ Windows-only WPF | ✅ macOS/Windows/Linux |
| Plugin sandbox | ⚠️ .dll with AssemblyLoadContext | ✅ WASM with wasmtime |
| Test coverage | Unknown | 73 unit tests, all passing |

---

## Recommended Attack Order

### Sprint 1 — Fix the Broken Things (P0)
✅ Done: Exit lockup, Go button, DND reordering

### Sprint 2 — Playback Essentials (P1.1–P1.5)
✅ Done: Delay, Playback progress, Looping, Preload, Cue state machine

### Sprint 3 — Cue List Density (P1.6–P1.7)
✅ Done: More columns, Inline editing

### Sprint 4 — Audio Engine Depth (P1.8–P1.10)
8. **P1.10 Fade processor wiring** — ✅ Done: fade-in via `FadeProcessor`, fade-out/volume via `MixerInput::start_fade()`
9. **P1.9 Double-buffered reading** — add `AudioFileReader` with ring buffer
10. **P1.8 Full limiter** — implement lookahead limiter with GR metering

### Sprint 5 — Windows & Menus (P1.11–P1.13, P2)
11. **P1.13 Complete menu bar** — ✅ Done
12. **P1.12 Log window** — ✅ Done
13. **P1.11 Remote nodes window** — OSC remote node editor
14. **P2.1 Pack Project** — self-contained media distribution

---

*Last updated: 2026-04-22*
