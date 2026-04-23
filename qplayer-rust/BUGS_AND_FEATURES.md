# QPlayer Rust Port — Known Issues & Feature Roadmap

> **Project:** `qplayer-rust`  
> **Workspace:** `crates/qplayer-*` (core, audio, video, gui, protocols, plugin-api) + `qplayer` binary  
> **Last updated:** 2026-04-22

## ✅ Recently Fixed (2026-04-22)

| # | Issue | Commit |
|:---|:---|:---|
| 2 | Cue enable toggle has no effect | Added `cue.enabled()` guards in `play_cue`, `handle_go`, TimeCode trigger |
| 9 | Error on video with no audio | Downgraded `StreamNotFound` log from `ERROR` → `INFO` |
| 1 | Pause doesn't pause video | Freeze `frame_counter` when paused; added `video_pause_flag` to decode thread |
| 3 | QID field not editable | Added stable `id_salt` to inspector QID `TextEdit` |
| 4 | Audio garbled on pcm_f32le WAV | Derive channel layout when undefined; bypass SwrContext for f32-packed sources |
| 8 | Looping video only loops audio | Added `loop_counter` to `LoopProcessor`; restart video thread + reset GUI progress on loop |
| 5 | UI column alignment / width | Unified `COL_*` width constants in cue list header + body |

## 🚫 Not Bugs (Already Working)

| # | Issue | Status |
|:---|:---|:---|
| 6 | Cue name can only be changed in inspector | ✅ Inline editing already works in cue list |
| 7 | No way to delete cue | ✅ Works via right-click context menu and Delete key |

---

## 🔴 KNOWN ISSUES

### 1. ~~Pause does not pause video playback, but does pause audio~~ ✅ FIXED
| | |
|:---|:---|
| **Severity** | High |
| **Affected crates** | `qplayer-video`, `qplayer-audio`, `qplayer` (main loop) |
| **Fix** | `frame_counter` in `mixer.rs` now only advances when at least one input is active. Added `video_pause_flag: Arc<AtomicBool>` that stops the decode thread from reading frames during pause. |

---

### 2. ~~Cue enable toggle has no effect~~ ✅ FIXED
| | |
|:---|:---|
| **Severity** | High |
| **Affected crates** | `qplayer-core`, `qplayer-gui` |
| **Fix** | Added `if !cue.enabled() { return; }` guard at the top of `play_cue()`. Also filters disabled cues in `handle_go()` and TimeCode trigger logic. |

---

### 3. ~~QID field not editable~~ ✅ FIXED
| | |
|:---|:---|
| **Severity** | Medium |
| **Affected crates** | `qplayer-gui` (inspector) |
| **Fix** | Added `id_salt` to the inspector QID `TextEdit` so egui retains focus across frames. The field was already editable in code but the unstable widget ID prevented interaction. |

---

### 4. ~~Audio playback garbled on some PCM WAV files~~ ✅ FIXED
| | |
|:---|:---|
| **Severity** | High |
| **Affected crates** | `qplayer-audio` (decoder / resampler) |
| **Sample file metadata** | `FREDERICK_90.09_03.wav_L-DlgMtch_01.wav` <br>• Format: WAV <br>• Codec: `pcm_f32le` ([3][0][0][0] / 0x0003) <br>• Sample rate: 48 000 Hz <br>• Channels: 1 (mono) <br>• Bitrate: 1536 kb/s |
| **Fix** | Pro Tools exports often leave `channel_layout = 0` (undefined). SwrContext misbehaves with undefined layouts. Now deriving `ChannelLayout::default(channels)` when `bits() == 0`. Also bypass SwrContext entirely when source is already `F32(Packed)` — no conversion needed. |

---

### 5. ~~Frontend UI — cue column names don't align with details, and cue width expands unexpectedly~~ ✅ FIXED
| | |
|:---|:---|
| **Severity** | Medium |
| **Affected crates** | `qplayer-gui` (cue list) |
| **Fix** | Defined `COL_*` width constants and applied `ui.add_sized()` consistently to every header label and body widget. The drag handle in edit mode now has a matching spacer in the header. ComboBox is constrained inside a sized container so it can't expand the row. |

---

### 6. ~~Cue name can only be changed in inspector~~ ✅ ALREADY WORKS
| | |
|:---|:---|
| **Severity** | Low (UX) |
| **Status** | Inline name editing exists in the cue list (Edit mode). Click the name cell to edit. |

---

### 7. ~~No way to delete a cue~~ ✅ ALREADY WORKS
| | |
|:---|:---|
| **Severity** | Medium |
| **Status** | Right-click any cue row → **Delete**. Also works with the `Delete` / `Backspace` key. |

---

### 8. ~~Looping video only loops embedded audio; playback indicators do not reset~~ ✅ FIXED
| | |
|:---|:---|
| **Severity** | High |
| **Affected crates** | `qplayer-audio` (`loop_processor.rs`), `qplayer-video`, `qplayer-gui` |
| **Fix** | Added `loop_counter: Arc<AtomicU32>` to `LoopProcessor` that increments on each loop. Main thread polls the counter for the active video cue and restarts the decode thread when it changes. GUI sync code now computes loop-relative position (`total_frames % loop_length`) so progress bars snap back to 0. |

---

### 9. ~~Error logged when video file has no embedded audio stream~~ ✅ FIXED
| | |
|:---|:---|
| **Severity** | Low (noise) |
| **Affected crates** | `qplayer-audio` (decoder), `qplayer` |
| **Fix** | Match `FfmpegError::StreamNotFound` in `play_audio()` and log at `INFO` level instead of `ERROR`. Added `qplayer_audio::FfmpegError` re-export so the main binary can match FFmpeg errors. |

---

## 🟢 FEATURES TO ADD

### 1. Audio matrix
| | |
|:---|:---|
| **Priority** | High |
| **Affected crates** | `qplayer-audio` (mixer), `qplayer-gui` (inspector) |
| **Description** | A per-cue routing matrix that maps input channels to output channels with gain control. Essential for multi-channel shows (theatre, installed sound, spatial audio). |
| **Technical notes** | • The mixer already processes per-cue pan/gain in `mixer.rs`. Extend `CueAudioSettings` with a `matrix: Vec<Vec<f32>>` (input ch × output ch).  <br>• Apply the matrix after decoding / resampling but before the fader/limiter chain.  <br>• In the GUI, provide a small grid widget (sliders or dB spinners) in the cue inspector.  <br>• Presets: Mono→Stereo, Stereo→L/R, 5.1 down-mix, etc. |

---

### 2. Video mapping / projection mapping
| | |
|:---|:---|
| **Priority** | High |
| **Affected crates** | `qplayer-video` (renderer, window), `qplayer-gui` |
| **Description** | Allow video outputs to be positioned, scaled, rotated, and corner-pinned across multiple displays or within a single window. Needed for non-rectangular surfaces, blended edge projections, and LED wall mapping. |
| **Technical notes** | • The video renderer currently uses a fullscreen blit (`blit.wgsl`). Replace with a textured quad pipeline that accepts a 4×3 homography / corner-pin matrix.  <br>• Extend `VideoOutputSettings` with `transform: VideoTransform { x, y, scale_x, scale_y, rotation, corner_pin: [Vec2; 4] }`.  <br>• Support multiple `VideoWindow`s (already stubbed in `window.rs`) so a single cue can drive multiple outputs with independent transforms.  <br>• In the GUI, add a *Video Mapping* tab with a preview canvas where users can drag corner handles. |

---

### 3. *(Placeholder — user left blank)*
| | |
|:---|:---|
| **Priority** | — |
| **Description** | *(Awaiting further requirements.)* |

---

## 🛠 Developer Quick-Reference

| Crate | Responsibility | Key files for issues above |
|:---|:---|:---|
| `qplayer` | Main event loop, OSC/MIDI routing, showfile I/O | `src/main.rs` |
| `qplayer-core` | Cue model, showfile schema, migrations | `src/cue/mod.rs`, `src/showfile/mod.rs` |
| `qplayer-audio` | FFmpeg decode, resample, effects, mixer, loop | `src/decoder.rs`, `src/loop_processor.rs`, `src/mixer.rs` |
| `qplayer-video` | FFmpeg video decode, GPU texture, window | `src/video_source.rs`, `src/video_output.rs`, `src/renderer.rs`, `src/window.rs` |
| `qplayer-gui` | egui panels: cue list, inspector, transport, active cues | `src/cue_list/mod.rs`, `src/inspector/mod.rs`, `src/transport/mod.rs` |
| `qplayer-protocols` | OSC, MSC, network input | `src/osc/mod.rs` |

---

## How to contribute

1. Pick an issue above.
2. Open a draft PR referencing this file (`BUGS_AND_FEATURES.md`).
3. Update the checklist/status in this doc when the fix is merged.

*Generated from user notes and codebase inspection — please keep in sync as bugs are fixed or new ones are discovered.*
