//! Video output crate — fullscreen GPU window for video playback.
//!
//! Architecture:
//! - `OutputWindow`: owns a winit window + wgpu surface (borderless fullscreen).
//! - `Renderer`: simple textured quad blit pipeline.
//! - `Texture`: double-buffered RGBA texture upload from CPU-decoded frames.
//! - `VideoSource`: wraps FFmpeg video decoder + `sws_scale` converter.
//!
//! A/V sync: the audio clock (from `qplayer-audio`) is the master. Video frames
//! are presented only when their PTS <= audio clock + vsync offset.

mod renderer;
mod texture;
mod video_source;
mod window;

pub use renderer::Renderer;
pub use texture::{Texture, VideoFrame};
pub use video_source::VideoSource;
pub use window::OutputWindow;
