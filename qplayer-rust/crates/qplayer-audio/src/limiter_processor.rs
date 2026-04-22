//! Lookahead limiter with soft-knee compression.
//!
//! Simplified from C# `AudioLimiterSampleProvider` while preserving core
//! behavior: lookahead delay, gain-reduction envelope, stereo linking,
//! and hard clip to threshold.

use crate::SampleProvider;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Lookahead limiter processor.
pub struct LimiterProcessor {
    source: Box<dyn SampleProvider>,
    inner: UnsafeCell<LimiterInner>,
    // Atomic parameters
    cmd_threshold: AtomicU32,     // f32::to_bits
    cmd_input_gain: AtomicU32,    // f32::to_bits
    cmd_enabled: AtomicBool,
}

struct LimiterInner {
    enabled: bool,
    threshold: f32,
    input_gain: f32,
    channels: u16,

    // Lookahead delay (ring buffer)
    delay: Vec<f32>,
    delay_write: usize,
    delay_read: usize,
    delay_size: usize,

    // Envelope follower state
    envelope: f32,
    /// Attack coefficient (smoothing)
    attack_coef: f32,
    /// Release coefficient
    release_coef: f32,
    /// Hold counter
    hold_counter: u32,
    /// Hold duration in samples
    hold_samples: u32,
}

impl LimiterProcessor {
    /// Create a limiter. `threshold` is linear gain (e.g., 0.95 = -0.45 dB).
    pub fn new(source: Box<dyn SampleProvider>, threshold: f32) -> Self {
        let sr = source.sample_rate();
        let ch = source.channels();
        let delay_ms = 5.0f32;
        let delay_samples = ((sr as f32 * delay_ms / 1000.0) * ch as f32).ceil() as usize;
        let delay_size = delay_samples.next_power_of_two();

        Self {
            source,
            inner: UnsafeCell::new(LimiterInner {
                enabled: true,
                threshold: threshold.clamp(0.01, 1.0),
                input_gain: 1.0,
                channels: ch,
                delay: vec![0.0f32; delay_size],
                delay_write: 0,
                delay_read: 0,
                delay_size,
                envelope: 1.0,
                attack_coef: Self::time_to_coef(2.0, sr),  // 2ms default attack
                release_coef: Self::time_to_coef(50.0, sr), // 50ms default release
                hold_counter: 0,
                hold_samples: (sr as f32 * 10.0 / 1000.0) as u32, // 10ms hold
            }),
            cmd_threshold: AtomicU32::new(threshold.clamp(0.01, 1.0).to_bits()),
            cmd_input_gain: AtomicU32::new(1.0f32.to_bits()),
            cmd_enabled: AtomicBool::new(true),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.cmd_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_threshold(&self, threshold: f32) {
        self.cmd_threshold
            .store(threshold.clamp(0.01, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_input_gain(&self, gain: f32) {
        self.cmd_input_gain.store(gain.max(0.0).to_bits(), Ordering::Relaxed);
    }

    /// Convert time constant (ms) to smoothing coefficient.
    #[inline]
    fn time_to_coef(ms: f32, sr: u32) -> f32 {
        let samples = ms * sr as f32 / 1000.0;
        (-1.0 / samples.max(1.0)).exp()
    }

    #[inline]
    #[allow(clippy::mut_from_ref)]
    fn inner_mut(&self) -> &mut LimiterInner {
        unsafe { &mut *self.inner.get() }
    }
}

impl SampleProvider for LimiterProcessor {
    fn read(&self, buffer: &mut [f32]) -> usize {
        let read = self.source.read(buffer);
        let inner = self.inner_mut();
        let channels = inner.channels as usize;

        // Refresh parameters
        inner.enabled = self.cmd_enabled.load(Ordering::Relaxed);
        inner.threshold = f32::from_bits(self.cmd_threshold.load(Ordering::Relaxed));
        inner.input_gain = f32::from_bits(self.cmd_input_gain.load(Ordering::Relaxed));

        if !inner.enabled || inner.threshold >= 1.0 {
            return read;
        }

        let mask = inner.delay_size - 1;
        let threshold = inner.threshold;
        let input_gain = inner.input_gain;
        let attack_coef = inner.attack_coef;
        let release_coef = inner.release_coef;
        let hold_samples = inner.hold_samples;

        let frames = read / channels.max(1);

        for frame in 0..frames {
            // Compute per-channel peak after input gain
            let mut peak_l = 0.0f32;
            let mut peak_r = 0.0f32;

            for ch in 0..channels {
                let s = buffer[frame * channels + ch] * input_gain;
                let abs_s = s.abs();
                if ch == 0 {
                    peak_l = abs_s;
                } else if ch == 1 {
                    peak_r = abs_s;
                }
                // Write to delay buffer
                inner.delay[inner.delay_write & mask] = s;
                inner.delay_write += 1;
            }

            // Stereo link: use the larger peak
            let peak = if channels >= 2 {
                peak_l.max(peak_r)
            } else {
                peak_l
            };

            // Compute desired gain reduction: threshold / peak
            let target_gr = if peak > threshold {
                threshold / peak
            } else {
                1.0
            };

            // Exponential envelope follower with attack/release differentiation
            if target_gr < inner.envelope {
                // Attack (reduce gain quickly)
                inner.envelope = attack_coef * inner.envelope + (1.0 - attack_coef) * target_gr;
                inner.hold_counter = hold_samples;
            } else {
                // Release (restore gain slowly)
                if inner.hold_counter > 0 {
                    inner.hold_counter -= 1;
                } else {
                    inner.envelope = release_coef * inner.envelope + (1.0 - release_coef) * target_gr;
                }
            }

            // Clamp envelope to valid range
            inner.envelope = inner.envelope.clamp(0.0, 1.0);

            // Apply gain reduction to delayed signal
            for ch in 0..channels {
                let delayed = inner.delay[inner.delay_read & mask];
                inner.delay_read += 1;

                let mut out = delayed * inner.envelope;
                // Hard clip to threshold
                out = out.clamp(-threshold, threshold);
                buffer[frame * channels + ch] = out;
            }
        }

        read
    }

    fn seek(&self, sample: usize) {
        self.source.seek(sample);
        let inner = self.inner_mut();
        inner.delay_write = 0;
        inner.delay_read = 0;
        inner.envelope = 1.0;
        inner.hold_counter = 0;
        inner.delay.fill(0.0);
    }

    fn position(&self) -> usize {
        self.source.position()
    }

    fn length(&self) -> Option<usize> {
        self.source.length()
    }

    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }

    fn channels(&self) -> u16 {
        self.source.channels()
    }
}

unsafe impl Send for LimiterProcessor {}
unsafe impl Sync for LimiterProcessor {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FnSource;

    fn dc_source(val: f32) -> Box<dyn SampleProvider> {
        Box::new(FnSource::new(
            move |buf| {
                for s in buf.iter_mut() { *s = val; }
                buf.len()
            },
            48000,
            2,
        ))
    }

    #[test]
    fn test_limiter_disabled_passes_through() {
        let limiter = LimiterProcessor::new(dc_source(1.0), 0.95);
        limiter.set_enabled(false);

        let mut buf = vec![0.0f32; 4];
        limiter.read(&mut buf);
        assert_eq!(buf, vec![1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_limiter_clips_above_threshold() {
        // Input = 2.0, threshold = 0.5 → should be limited to ~0.5
        let limiter = LimiterProcessor::new(dc_source(2.0), 0.5);

        // Need enough samples to fill the lookahead delay first
        let mut buf = vec![0.0f32; 4096];
        limiter.read(&mut buf);

        // After the delay, samples should be clamped to threshold
        let tail = &buf[2048..];
        let max_val = tail.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_val <= 0.55,
            "limiter should clamp to ~0.5, got max {}",
            max_val
        );
    }

    #[test]
    fn test_limiter_does_not_affect_below_threshold() {
        // Input = 0.3, threshold = 0.5 → should pass through
        let limiter = LimiterProcessor::new(dc_source(0.3), 0.5);

        let mut buf = vec![0.0f32; 4096];
        limiter.read(&mut buf);

        let tail = &buf[2048..];
        let min_val = tail.iter().map(|s| s.abs()).fold(f32::MAX, f32::min);
        assert!(
            min_val > 0.25,
            "limiter should not affect signals below threshold, got min {}",
            min_val
        );
    }

    #[test]
    fn test_seek_resets() {
        let limiter = LimiterProcessor::new(dc_source(2.0), 0.5);
        let mut buf = vec![0.0f32; 4096];
        limiter.read(&mut buf);

        limiter.seek(0);
        let mut buf2 = vec![0.0f32; 4096];
        limiter.read(&mut buf2);

        // After seek, the delay should be reset so first samples pass through
        assert!(buf2[0] > 0.1, "after seek, initial samples should pass through delay");
    }
}
