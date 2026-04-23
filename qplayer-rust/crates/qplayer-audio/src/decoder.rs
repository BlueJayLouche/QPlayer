//! FFmpeg-based audio file decoder.
//!
//! Opens any format FFmpeg supports (WAV, MP3, FLAC, OGG, AIFF, WMA, ...).
//! Uses SwrContext to convert any native sample format to interleaved f32.

use crate::SampleProvider;
use ffmpeg_next::{self as ffmpeg, codec, format, frame, media, util::mathematics::rescale::Rescale};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// FFmpeg-based audio file decoder.
pub struct FfmpegDecoder {
    inner: UnsafeCell<FfmpegDecoderInner>,
}

struct FfmpegDecoderInner {
    ictx: format::context::Input,
    decoder: codec::decoder::Audio,
    stream_index: usize,
    time_base: ffmpeg::Rational,
    sample_rate: u32,
    channels: u16,
    total_samples: Option<usize>,
    position: AtomicUsize,
    decoded_frame: frame::Audio,
    swr: Option<ffmpeg::software::resampling::context::Context>,
    swr_out: frame::Audio,
    /// Residual converted samples not yet consumed by read().
    residual: Vec<f32>,
    eof: AtomicUsize,
}

impl FfmpegDecoder {
    pub fn open(path: &str) -> Result<Self, ffmpeg::Error> {
        let ictx = format::input(path)?;
        let input = ictx
            .streams()
            .best(media::Type::Audio)
            .ok_or(ffmpeg::Error::StreamNotFound)?;

        let stream_index = input.index();
        let time_base = input.time_base();
        let context = codec::context::Context::from_parameters(input.parameters())?;
        let decoder = context.decoder().audio()?;

        let sample_rate = decoder.rate() as u32;
        let channels = decoder.channels() as u16;
        let channel_layout = decoder.channel_layout();
        // Pro Tools and some exporters leave channel_layout as 0 (undefined).
        // SwrContext misbehaves with an undefined layout, so derive one from the channel count.
        let channel_layout = if channel_layout.bits() == 0 {
            log::debug!("Undefined channel layout for {}; deriving from {} channels", path, channels);
            ffmpeg::ChannelLayout::default(channels as i32)
        } else {
            channel_layout
        };

        let duration = input.duration();
        let total_samples = if duration > 0 {
            let samples = duration.rescale(time_base, (1, sample_rate as i32)) as usize;
            Some(samples * channels as usize)
        } else {
            None
        };

        // SwrContext: convert decoder's native format → f32 packed (interleaved).
        let swr = match ffmpeg::software::resampling::context::Context::get(
            decoder.format(),
            channel_layout,
            sample_rate,
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
            channel_layout,
            sample_rate,
        ) {
            Ok(ctx) => Some(ctx),
            Err(e) => {
                log::warn!("SwrContext init failed (format={:?}, layout={:?}, sr={}): {}. Will use fallback.",
                    decoder.format(), channel_layout, sample_rate, e);
                None
            }
        };

        let swr_out = frame::Audio::empty();
        let decoded_frame = frame::Audio::empty();

        Ok(Self {
            inner: UnsafeCell::new(FfmpegDecoderInner {
                ictx,
                decoder,
                stream_index,
                time_base,
                sample_rate,
                channels,
                total_samples,
                position: AtomicUsize::new(0),
                decoded_frame,
                swr,
                swr_out,
                residual: Vec::new(),
                eof: AtomicUsize::new(0),
            }),
        })
    }

    #[inline]
    #[allow(clippy::mut_from_ref)]
    fn inner_mut(&self) -> &mut FfmpegDecoderInner {
        unsafe { &mut *self.inner.get() }
    }
}

impl FfmpegDecoderInner {
    fn decode_into(&mut self, buffer: &mut [f32]) -> usize {
        let mut written = 0;

        // Drain residual samples first
        let residual_len = self.residual.len();
        if residual_len > 0 {
            let to_copy = residual_len.min(buffer.len());
            buffer[..to_copy].copy_from_slice(&self.residual[..to_copy]);
            written += to_copy;
            self.residual.drain(..to_copy);
            if written >= buffer.len() {
                return written;
            }
        }

        while written < buffer.len() && self.eof.load(Ordering::Relaxed) == 0 {
            match self.decoder.receive_frame(&mut self.decoded_frame) {
                Ok(()) => {
                    if let Some(ref mut swr) = self.swr {
                        // Convert decoded frame to f32 packed via SwrContext
                        match swr.run(&self.decoded_frame, &mut self.swr_out) {
                            Ok(_) => {
                                let copied = self.drain_swr_out(&mut buffer[written..]);
                                written += copied;
                            }
                            Err(e) => {
                                log::warn!("SWR convert error: {}, falling back to manual", e);
                                let samples = self.write_frame_fallback(&mut buffer[written..]);
                                written += samples;
                            }
                        }
                    } else {
                        let samples = self.write_frame_fallback(&mut buffer[written..]);
                        written += samples;
                    }
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                    if let Some(packet) = self.read_packet() {
                        if let Err(e) = self.decoder.send_packet(&packet) {
                            log::warn!("Send packet error: {}", e);
                        }
                    } else {
                        let _ = self.decoder.send_eof();
                        if let Some(ref mut swr) = self.swr {
                            let _ = swr.flush(&mut self.swr_out);
                        }
                        // Drain any final SWR output
                        if let Some(ref mut _swr) = self.swr {
                            let copied = self.drain_swr_out(&mut buffer[written..]);
                            written += copied;
                        }
                        self.eof.store(1, Ordering::Relaxed);
                    }
                }
                Err(ffmpeg::Error::Eof) => {
                    self.eof.store(1, Ordering::Relaxed);
                    break;
                }
                Err(e) => {
                    log::warn!("Decode error: {}", e);
                    break;
                }
            }
        }

        written
    }

    /// Copy samples from swr_out frame to buffer. Any excess goes into residual.
    fn drain_swr_out(&mut self, buffer: &mut [f32]) -> usize {
        let frame = &self.swr_out;
        let is_empty = unsafe { frame.is_empty() };
        if is_empty {
            return 0;
        }
        // Use the exact sample count, not plane.len(), to avoid reading FFmpeg alignment padding.
        let nb_samples = frame.samples() * frame.channels() as usize;
        if nb_samples == 0 {
            self.swr_out = frame::Audio::empty();
            return 0;
        }
        let plane = frame.data(0);
        let src = unsafe {
            std::slice::from_raw_parts(plane.as_ptr() as *const f32, nb_samples)
        };
        let to_copy = src.len().min(buffer.len());
        buffer[..to_copy].copy_from_slice(&src[..to_copy]);

        if to_copy < src.len() {
            self.residual.extend_from_slice(&src[to_copy..]);
        }
        self.swr_out = frame::Audio::empty();

        to_copy
    }

    /// Fallback for when SwrContext is unavailable (should not normally happen).
    fn write_frame_fallback(&self, buffer: &mut [f32]) -> usize {
        let frame = &self.decoded_frame;
        let format = frame.format();
        let channels = frame.channels() as usize;
        let frame_samples = frame.samples() * channels;
        let to_write = frame_samples.min(buffer.len());

        match format {
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed) => {
                let plane = frame.data(0);
                let src = unsafe {
                    std::slice::from_raw_parts(
                        plane.as_ptr() as *const f32,
                        plane.len() / std::mem::size_of::<f32>(),
                    )
                };
                buffer[..to_write].copy_from_slice(&src[..to_write]);
            }
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Planar) => {
                for ch in 0..channels {
                    let plane = frame.data(ch);
                    let src = unsafe {
                        std::slice::from_raw_parts(
                            plane.as_ptr() as *const f32,
                            plane.len() / std::mem::size_of::<f32>(),
                        )
                    };
                    let frames = to_write / channels;
                    for i in 0..frames {
                        buffer[i * channels + ch] = src[i];
                    }
                }
            }
            ffmpeg::format::Sample::I16(ffmpeg::format::sample::Type::Packed) => {
                let plane = frame.data(0);
                let src = unsafe {
                    std::slice::from_raw_parts(
                        plane.as_ptr() as *const i16,
                        plane.len() / std::mem::size_of::<i16>(),
                    )
                };
                for i in 0..to_write {
                    buffer[i] = src[i] as f32 / 32768.0;
                }
            }
            ffmpeg::format::Sample::I32(ffmpeg::format::sample::Type::Packed) => {
                let plane = frame.data(0);
                let src = unsafe {
                    std::slice::from_raw_parts(
                        plane.as_ptr() as *const i32,
                        plane.len() / std::mem::size_of::<i32>(),
                    )
                };
                for i in 0..to_write {
                    buffer[i] = src[i] as f32 / 2147483648.0;
                }
            }
            ffmpeg::format::Sample::I32(ffmpeg::format::sample::Type::Planar) => {
                for ch in 0..channels {
                    let plane = frame.data(ch);
                    let src = unsafe {
                        std::slice::from_raw_parts(
                            plane.as_ptr() as *const i32,
                            plane.len() / std::mem::size_of::<i32>(),
                        )
                    };
                    let frames = to_write / channels;
                    for i in 0..frames {
                        buffer[i * channels + ch] = src[i] as f32 / 2147483648.0;
                    }
                }
            }
            _ => {
                log::warn!("Unhandled sample format {:?}, output will be silent", format);
                for sample in &mut buffer[..to_write] {
                    *sample = 0.0;
                }
            }
        }

        to_write
    }

    fn read_packet(&mut self) -> Option<ffmpeg::Packet> {
        for (stream, packet) in self.ictx.packets() {
            if stream.index() == self.stream_index {
                return Some(packet);
            }
        }
        None
    }
}

impl SampleProvider for FfmpegDecoder {
    fn read(&self, buffer: &mut [f32]) -> usize {
        let read = self.inner_mut().decode_into(buffer);
        self.inner_mut().position.fetch_add(read, Ordering::Relaxed);
        read
    }

    fn seek(&self, sample: usize) {
        let inner = self.inner_mut();
        let timestamp = (sample / inner.channels as usize) as i64;
        let ts = timestamp.rescale((1, inner.sample_rate as i32), inner.time_base);

        if let Err(e) = inner.ictx.seek(ts, ..) {
            log::warn!("Seek error: {}", e);
            return;
        }

        inner.decoder.flush();
        inner.eof.store(0, Ordering::Relaxed);
        inner.position.store(sample, Ordering::Relaxed);

        // Reset SWR state
        if let Some(ref mut swr) = inner.swr {
            let _ = swr.flush(&mut inner.swr_out);
        }
        inner.swr_out = frame::Audio::empty();
        inner.residual.clear();
    }

    fn position(&self) -> usize {
        self.inner_mut().position.load(Ordering::Relaxed)
    }

    fn length(&self) -> Option<usize> {
        self.inner_mut().total_samples
    }

    fn sample_rate(&self) -> u32 {
        self.inner_mut().sample_rate
    }

    fn channels(&self) -> u16 {
        self.inner_mut().channels
    }
}

unsafe impl Send for FfmpegDecoder {}
unsafe impl Sync for FfmpegDecoder {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_ping() {
        let decoder = FfmpegDecoder::open("/System/Library/Sounds/Ping.aiff").unwrap();
        assert_eq!(decoder.sample_rate(), 48000);
        assert_eq!(decoder.channels(), 2);
        assert!(decoder.length().unwrap() > 0);
    }

    #[test]
    fn test_decode_ping() {
        let decoder = FfmpegDecoder::open("/System/Library/Sounds/Ping.aiff").unwrap();
        let mut buf = vec![0.0f32; 48000 * 2]; // 1 second
        let read = decoder.read(&mut buf);
        assert!(read > 0, "should decode some samples");

        // Check that samples are in valid f32 range (-1.0 to 1.0)
        let max = buf[..read].iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max > 0.001 && max <= 1.0,
            "decoded samples should be in [-1, 1] range, got max {}",
            max
        );
    }

    #[test]
    fn test_decode_produces_reasonable_waveform() {
        let decoder = FfmpegDecoder::open("/System/Library/Sounds/Ping.aiff").unwrap();
        let mut buf = vec![0.0f32; 48000 * 2];
        let read = decoder.read(&mut buf);

        // A "ping" sound should have some zero crossings and variation
        let mut zero_crossings = 0;
        for i in 1..read {
            if buf[i - 1] * buf[i] < 0.0 {
                zero_crossings += 1;
            }
        }
        assert!(
            zero_crossings > 100,
            "a real audio signal should have many zero crossings, got {}",
            zero_crossings
        );
    }
}
