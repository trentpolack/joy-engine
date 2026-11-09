// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Offline procedural sound synthesis: deterministic building blocks games render short cues from, once, into mono PCM
//! for the [`AudioBank`](crate::AudioBank).
//!
//! Two families share one sample rate per [`Synth`]:
//!
//! - **Layers** ([`Synth::noise_burst`], [`Synth::bent_tone`], [`Synth::sweep`], [`Synth::filtered_sweep_noise`],
//!   [`Synth::rumble`], [`Synth::crackle`], [`Synth::click`], [`Synth::beep`], [`tremolo`]) return whole `f32` buffers a
//!   recipe sums with [`Synth::mix_normalized`]. Noise comes from a caller's seeded [`RandomSource`], so a cue sounds
//!   the same on every target and never draws from gameplay streams.
//! - **Voices** ([`VoiceMix::tone`], [`VoiceMix::burst`], [`VoiceMix::arpeggio`]) schedule band-limited oscillators
//!   ([`Waveform`], with polyBLEP square and sawtooth) and filtered noise at start times, with the exponential ramps,
//!   attack-release envelope, and `BiquadFilterNode` coefficients Web Audio uses, so recipes written against browser
//!   nodes render the same offline.
//!
//! Durations are seconds, frequencies hertz, decay rates per second, and gains linear. Every transcendental call goes
//! through `math::deterministic`.

use std::f64::consts::PI;

use joy_core::RandomSource;
use joy_core::math::deterministic;

/// Envelope floor: Web Audio exponential ramps cannot reach zero.
const ENVELOPE_FLOOR: f64 = 0.0001;
/// Smallest envelope peak, so a ramp never targets the floor itself.
const MINIMUM_PEAK: f64 = 0.0002;
/// Longest envelope attack, in seconds.
const MAXIMUM_ATTACK_SECONDS: f64 = 0.012;
/// Fraction of a voice an attack may take at most.
const ATTACK_FRACTION: f64 = 0.25;
/// A voice keeps rendering this long after its envelope ends, like a scheduled source stopping slightly late.
const STOP_PADDING_SECONDS: f64 = 0.05;
/// Biquad Q: a resonance in decibels for low- and high-pass, linear for band-pass (Web Audio's default of one).
const DEFAULT_Q: f64 = 1.0;
/// Lowest glide target in hertz.
const MINIMUM_FREQUENCY: f64 = 20.0;
/// Low-pass cutoff of arpeggio notes, in hertz.
const ARPEGGIO_LOWPASS: f64 = 2600.0;
/// Seed of the voices' shared noise buffer (a fixed linear congruential sequence).
const NOISE_SEED: u32 = 0x0123_4567;

// ---------------------------------------------------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------------------------------------------------

/// Renders layers at one sample rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Synth {
    rate: f64,
}

impl Synth {
    /// A synthesizer at `sample_rate` hertz.
    pub const fn new(sample_rate: u32) -> Self {
        Self {
            rate: sample_rate as f64,
        }
    }

    /// The sample rate in hertz.
    pub fn sample_rate(&self) -> f64 {
        self.rate
    }

    /// Samples in `seconds`, rounded, and at least one.
    pub fn samples(&self, seconds: f64) -> usize {
        f64::round(seconds * self.rate).max(1.0) as usize
    }

    /// A silent buffer `seconds` long.
    pub fn silence(&self, seconds: f64) -> Vec<f32> {
        vec![0.0; self.samples(seconds)]
    }

    /// Sum `layers` into a buffer `seconds` long, scale it down when it peaks above `peak`, and fade its last
    /// `fade_seconds` out against clicks.
    pub fn mix_normalized(&self, seconds: f64, layers: &[Vec<f32>], peak: f64, fade_seconds: f64) -> Vec<f32> {
        let length = self.samples(seconds);
        let mut out = vec![0.0_f32; length];
        for layer in layers {
            for (sample, value) in out.iter_mut().zip(layer) {
                *sample += *value;
            }
        }
        let loudest = out.iter().fold(0.0_f64, |loudest, value| f64::max(loudest, f64::from(value.abs())));
        if loudest > peak {
            let scale = peak / loudest;
            for sample in &mut out {
                *sample = (f64::from(*sample) * scale) as f32;
            }
        }
        let fade = length.min(f64::round(fade_seconds * self.rate) as usize);
        for index in 0..fade {
            let sample = &mut out[length - 1 - index];
            *sample = (f64::from(*sample) * (index as f64 / fade as f64)) as f32;
        }
        out
    }

    /// Exponentially decaying noise, low-passed by a one-pole filter (`smoothing` 0 to 1, higher is brighter).
    pub fn noise_burst(
        &self,
        random: &mut impl RandomSource,
        seconds: f64,
        decay: f64,
        smoothing: f64,
        gain: f64,
    ) -> Vec<f32> {
        let mut out = self.silence(seconds);
        let mut filtered = 0.0;
        for (index, sample) in out.iter_mut().enumerate() {
            let time = index as f64 / self.rate;
            filtered += (signed_noise(random) - filtered) * smoothing;
            *sample = (filtered * deterministic::exp(-decay * time) * gain * 2.0) as f32;
        }
        out
    }

    /// A decaying sine whose pitch bends down by up to 40% over its first half second, for weight.
    pub fn bent_tone(&self, frequency: f64, seconds: f64, decay: f64, gain: f64) -> Vec<f32> {
        let mut out = self.silence(seconds);
        let mut phase = 0.0;
        for (index, sample) in out.iter_mut().enumerate() {
            let time = index as f64 / self.rate;
            phase += (PI * 2.0 * frequency * (1.0 - f64::min(0.4, time * 0.8))) / self.rate;
            *sample = (deterministic::sin(phase) * deterministic::exp(-decay * time) * gain) as f32;
        }
        out
    }

    /// An exponential frequency sweep from `from` to `to` hertz with decay.
    pub fn sweep(&self, from: f64, to: f64, seconds: f64, decay: f64, gain: f64) -> Vec<f32> {
        let mut out = self.silence(seconds);
        let length = out.len() as f64;
        let mut phase = 0.0;
        for (index, sample) in out.iter_mut().enumerate() {
            let t = index as f64 / length;
            let frequency = from * deterministic::powf(to / from, t);
            phase += (PI * 2.0 * frequency) / self.rate;
            *sample = (deterministic::sin(phase) * deterministic::exp(-decay * t * seconds) * gain) as f32;
        }
        out
    }

    /// Noise whose one-pole opening sweeps between two smoothings, with a quick rise and an exponential fall.
    pub fn filtered_sweep_noise(
        &self,
        random: &mut impl RandomSource,
        seconds: f64,
        from_smoothing: f64,
        to_smoothing: f64,
        decay: f64,
    ) -> Vec<f32> {
        let mut out = self.silence(seconds);
        let length = out.len() as f64;
        let mut filtered = 0.0;
        for (index, sample) in out.iter_mut().enumerate() {
            let t = index as f64 / length;
            let smoothing = from_smoothing + (to_smoothing - from_smoothing) * t;
            filtered += (signed_noise(random) - filtered) * smoothing;
            let envelope = f64::min(1.0, t * 12.0) * deterministic::exp(-decay * t * seconds);
            *sample = (filtered * envelope * 1.8) as f32;
        }
        out
    }

    /// A brown-noise rumble (leaky integrated white noise) decaying over its duration.
    pub fn rumble(&self, random: &mut impl RandomSource, seconds: f64, decay: f64, gain: f64) -> Vec<f32> {
        let mut out = self.silence(seconds);
        let mut brown = 0.0;
        for (index, sample) in out.iter_mut().enumerate() {
            let time = index as f64 / self.rate;
            brown = (brown + signed_noise(random) * 0.08) * 0.985;
            *sample = (brown * 3.5 * deterministic::exp(-decay * time) * gain * f64::min(1.0, time * 80.0)) as f32;
        }
        out
    }

    /// Sparse random clicks thinning out over the duration, for debris and electrical crackle.
    pub fn crackle(&self, random: &mut impl RandomSource, seconds: f64, gain: f64) -> Vec<f32> {
        let mut out = self.silence(seconds);
        let length = out.len();
        let click_length = f64::round(self.rate * 0.004) as usize;
        for index in 0..length {
            let t = index as f64 / length as f64;
            if random.next_unit() < 0.004 * (1.0 - t) {
                for offset in 0..click_length.min(length - index) {
                    let sample = &mut out[index + offset];
                    let value = signed_noise(random) * gain * (1.0 - offset as f64 / click_length as f64);
                    *sample = (f64::from(*sample) + value) as f32;
                }
            }
        }
        out
    }

    /// A metallic click (noise plus a 2.4 kHz ring) starting `start_seconds` in.
    pub fn click(&self, random: &mut impl RandomSource, start_seconds: f64, gain: f64) -> Vec<f32> {
        let mut out = self.silence(start_seconds + 0.06);
        let start = f64::round(start_seconds * self.rate) as usize;
        for (index, sample) in out.iter_mut().enumerate().skip(start) {
            let time = (index - start) as f64 / self.rate;
            let value = signed_noise(random) * 0.5 + deterministic::sin(time * PI * 2.0 * 2400.0) * 0.6;
            *sample = (value * deterministic::exp(-time * 90.0) * gain) as f32;
        }
        out
    }

    /// A square-ish (soft-clipped sine) beep with soft edges, starting `start_seconds` in and lasting `seconds`.
    pub fn beep(&self, frequency: f64, start_seconds: f64, seconds: f64, gain: f64) -> Vec<f32> {
        let mut out = self.silence(start_seconds + seconds);
        let start = f64::round(start_seconds * self.rate) as usize;
        for (index, sample) in out.iter_mut().enumerate().skip(start) {
            let time = (index - start) as f64 / self.rate;
            let edge = f64::min(1.0, f64::min(time * 400.0, (seconds - time) * 400.0));
            let shaped = deterministic::tanh(deterministic::sin(time * PI * 2.0 * frequency) * 3.0);
            *sample = (shaped * 0.5 * edge * gain) as f32;
        }
        out
    }
}

/// Amplitude modulation at `rate_hz` removing up to `depth` (0 to 1) of the layer, for a `sample_rate` layer.
pub fn tremolo(mut layer: Vec<f32>, rate_hz: f64, depth: f64, sample_rate: f64) -> Vec<f32> {
    for (index, sample) in layer.iter_mut().enumerate() {
        let gain = 1.0 - depth * (0.5 + 0.5 * deterministic::sin((index as f64 / sample_rate) * PI * 2.0 * rate_hz));
        *sample = (f64::from(*sample) * gain) as f32;
    }
    layer
}

/// Uniform noise in `[-1, 1)` from one draw.
fn signed_noise(random: &mut impl RandomSource) -> f64 {
    random.next_unit() * 2.0 - 1.0
}

// ---------------------------------------------------------------------------------------------------------------------
// Voices
// ---------------------------------------------------------------------------------------------------------------------

/// Oscillator waveforms; square and sawtooth are band-limited with polyBLEP corrections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    /// A sine.
    Sine,
    /// A triangle.
    Triangle,
    /// A square.
    Square,
    /// A rising sawtooth.
    Sawtooth,
}

/// Biquad filter responses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    /// Passes frequencies below the cutoff.
    Lowpass,
    /// Passes frequencies above the cutoff.
    Highpass,
    /// Passes a band around the center frequency.
    Bandpass,
}

/// Accumulates scheduled voices into one buffer at one sample rate.
#[derive(Clone, Debug)]
pub struct VoiceMix {
    rate: f64,
    samples: Vec<f64>,
    // The shared one-second noise buffer, built on first use.
    noise: Option<Vec<f64>>,
}

impl VoiceMix {
    /// An empty mix at `sample_rate` hertz.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            rate: f64::from(sample_rate),
            samples: Vec::new(),
            noise: None,
        }
    }

    /// An enveloped oscillator starting `start` seconds in, gliding exponentially from `from` to `to` hertz over
    /// `seconds` to a `peak` gain, low-passed at `lowpass` hertz when it is positive.
    #[allow(clippy::too_many_arguments, reason = "one voice's waveform, glide, timing, level, and filter")]
    pub fn tone(&mut self, wave: Waveform, from: f64, to: f64, start: f64, seconds: f64, peak: f64, lowpass: f64) {
        let rate = self.rate;
        let length = ((seconds + STOP_PADDING_SECONDS) * rate).ceil() as usize;
        let mut filter = (lowpass > 0.0).then(Biquad::default);
        let mut phase = 0.0;
        let mut output = Vec::with_capacity(length);
        for index in 0..length {
            let time = index as f64 / rate;
            let frequency = exponential_ramp(from, to.max(MINIMUM_FREQUENCY), seconds, time);
            let increment = frequency / rate;
            let mut value = oscillator(wave, phase, increment);
            phase = (phase + increment).fract();
            if let Some(filter) = &mut filter {
                filter.configure(FilterKind::Lowpass, lowpass, rate);
                value = filter.process(value);
            }
            output.push(value * percussive_envelope(time, seconds, peak));
        }
        self.mix(start, &output);
    }

    /// Filtered noise starting `start` seconds in, its cutoff gliding from `from_hz` to `to_hz`, for impacts,
    /// whooshes, and crackle. Bursts last at most one second (the noise buffer's length).
    pub fn burst(&mut self, start: f64, seconds: f64, peak: f64, filter: FilterKind, from_hz: f64, to_hz: f64) {
        let rate = self.rate;
        let noise = self.noise.get_or_insert_with(|| noise_buffer(rate as usize));
        let length = (((seconds + STOP_PADDING_SECONDS) * rate).ceil() as usize).min(noise.len());
        let mut biquad = Biquad::default();
        let mut output = Vec::with_capacity(length);
        for (index, sample) in noise.iter().take(length).enumerate() {
            let time = index as f64 / rate;
            biquad.configure(filter, exponential_ramp(from_hz, to_hz.max(MINIMUM_FREQUENCY), seconds, time), rate);
            output.push(biquad.process(*sample) * percussive_envelope(time, seconds, peak));
        }
        self.mix(start, &output);
    }

    /// `notes` (hertz) `spacing` seconds apart from `start`, each held for `sustain` and low-passed at 2.6 kHz.
    pub fn arpeggio(&mut self, wave: Waveform, notes: &[f64], start: f64, spacing: f64, sustain: f64, peak: f64) {
        for (index, note) in notes.iter().enumerate() {
            self.tone(wave, *note, *note, start + (index as f64 * spacing), sustain, peak, ARPEGGIO_LOWPASS);
        }
    }

    /// The mix, clamped to `[-1, 1]`.
    pub fn finish(self) -> Vec<f32> {
        self.samples.into_iter().map(|value| value.clamp(-1.0, 1.0) as f32).collect()
    }

    fn mix(&mut self, start: f64, voice: &[f64]) {
        let offset = (start * self.rate).round() as usize;
        if self.samples.len() < offset + voice.len() {
            self.samples.resize(offset + voice.len(), 0.0);
        }
        for (index, value) in voice.iter().enumerate() {
            self.samples[offset + index] += value;
        }
    }
}

/// Web Audio's `exponentialRampToValueAtTime` from `from` at zero to `to` at `seconds`, holding afterward.
pub fn exponential_ramp(from: f64, to: f64, seconds: f64, time: f64) -> f64 {
    if time >= seconds {
        return to;
    }
    from * deterministic::powf(to / from, time / seconds)
}

/// A fast attack and exponential release: from the floor to `peak` over `min(12 ms, 25%)`, back to the floor at
/// `seconds`, and silent after.
pub fn percussive_envelope(time: f64, seconds: f64, peak: f64) -> f64 {
    let peak = peak.max(MINIMUM_PEAK);
    let attack = MAXIMUM_ATTACK_SECONDS.min(seconds * ATTACK_FRACTION);
    if time < attack {
        return exponential_ramp(ENVELOPE_FLOOR, peak, attack, time);
    }
    if time < seconds {
        return exponential_ramp(peak, ENVELOPE_FLOOR, seconds - attack, time - attack);
    }
    ENVELOPE_FLOOR
}

/// One band-limited oscillator sample at `phase` (cycles) with a per-sample `increment` (cycles per sample).
pub fn oscillator(wave: Waveform, phase: f64, increment: f64) -> f64 {
    match wave {
        Waveform::Sine => deterministic::sin(phase * 2.0 * PI),
        Waveform::Triangle => 1.0 - (4.0 * (phase - 0.5).abs()),
        Waveform::Sawtooth => ((2.0 * phase) - 1.0) - poly_blep(phase, increment),
        Waveform::Square => {
            let square = if phase < 0.5 { 1.0 } else { -1.0 };
            square + poly_blep(phase, increment) - poly_blep((phase + 0.5).fract(), increment)
        }
    }
}

/// A direct-form biquad with Web Audio `BiquadFilterNode` coefficients.
#[derive(Clone, Debug, Default)]
pub struct Biquad {
    coefficients: [f64; 5],
    history: [f64; 4],
    configured: Option<(FilterKind, f64)>,
}

impl Biquad {
    /// Set the response and cutoff (or center) `frequency` in hertz for `rate`; unchanged settings keep the
    /// coefficients.
    pub fn configure(&mut self, filter: FilterKind, frequency: f64, rate: f64) {
        if self.configured == Some((filter, frequency)) {
            return;
        }
        self.configured = Some((filter, frequency));
        let omega = 2.0 * PI * (frequency / rate).min(0.5);
        let (sin, cos) = deterministic::sin_cos(omega);
        let [b0, b1, b2, a0, a1, a2] = match filter {
            FilterKind::Lowpass | FilterKind::Highpass => {
                // Low- and high-pass Q is a resonance in decibels.
                let alpha = sin / (2.0 * deterministic::powf(10.0_f64, DEFAULT_Q / 20.0));
                let (b0, b1) = if filter == FilterKind::Lowpass {
                    ((1.0 - cos) / 2.0, 1.0 - cos)
                } else {
                    ((1.0 + cos) / 2.0, -(1.0 + cos))
                };
                [b0, b1, b0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha]
            }
            FilterKind::Bandpass => {
                let alpha = sin / (2.0 * DEFAULT_Q);
                [alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha]
            }
        };
        self.coefficients = [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
    }

    /// Filter one sample.
    pub fn process(&mut self, input: f64) -> f64 {
        let [b0, b1, b2, a1, a2] = self.coefficients;
        let [x1, x2, y1, y2] = self.history;
        let output = (b0 * input) + (b1 * x1) + (b2 * x2) - (a1 * y1) - (a2 * y2);
        self.history = [input, x1, output, y1];
        output
    }
}

/// The polyBLEP correction near a waveform discontinuity.
fn poly_blep(phase: f64, increment: f64) -> f64 {
    if phase < increment {
        let t = phase / increment;
        return (t + t) - (t * t) - 1.0;
    }
    if phase > 1.0 - increment {
        let t = (phase - 1.0) / increment;
        return (t * t) + (t + t) + 1.0;
    }
    0.0
}

/// One second of noise in `[-1, 1)` from a fixed linear congruential sequence.
fn noise_buffer(length: usize) -> Vec<f64> {
    let mut state = NOISE_SEED;
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (f64::from(state) / 2_147_483_648.0) - 1.0
        })
        .collect()
}

#[cfg(test)]
#[path = "synth.test.rs"]
mod tests;
