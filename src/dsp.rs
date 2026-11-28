use std::collections::VecDeque;
use std::f64::consts::PI;

pub const FFT_SIZE: usize = 4096;

/// Direct Form I biquad filter with analytical frequency response evaluation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    pub sample_rate: f64,
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
    pub x1: f64,
    pub x2: f64,
    pub y1: f64,
    pub y2: f64,
}

impl Biquad {
    /// 2nd-order low-pass filter with cutoff frequency `fc` and Q factor.
    pub fn lowpass(fc: f64, q: f64, sample_rate: f64) -> Self {
        let sr = sample_rate.max(8.0);
        let nyquist = sr * 0.5;
        let fc = fc.clamp(1.0, nyquist - 1.0);
        let omega = 2.0 * PI * fc / sr;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * q.max(0.1));

        let a0 = 1.0 + alpha;
        let b0 = ((1.0 - cos_omega) * 0.5) / a0;
        let b1 = (1.0 - cos_omega) / a0;
        let b2 = ((1.0 - cos_omega) * 0.5) / a0;
        let a1 = (-2.0 * cos_omega) / a0;
        let a2 = (1.0 - alpha) / a0;

        Self {
            sample_rate: sr,
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Evaluates filter magnitude response in dB at `hz` using H(e^{jw}).
    pub fn gain_db_at(&self, hz: f64) -> f64 {
        let w = std::f64::consts::TAU * hz / self.sample_rate.max(1.0);
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let numerator = (self.b0 + self.b1 * cos1 + self.b2 * cos2).powi(2)
            + (self.b1 * sin1 + self.b2 * sin2).powi(2);
        let denominator = (1.0 + self.a1 * cos1 + self.a2 * cos2).powi(2)
            + (self.a1 * sin1 + self.a2 * sin2).powi(2);
        let ratio = (numerator / denominator.max(1e-24)).max(1e-12);
        10.0 * ratio.log10()
    }

    #[inline]
    pub fn run(&mut self, input: f64) -> f64 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

pub fn make_lr4_stage(cutoff_hz: f64, sample_rate: f64) -> Biquad {
    let cutoff = cutoff_hz.clamp(20.0, 1000.0);
    Biquad::lowpass(cutoff, std::f64::consts::FRAC_1_SQRT_2, sample_rate.max(1.0))
}

/// Evaluates the LR4 magnitude response in dB without locking the audio thread.
pub fn lr4_response_db(
    cutoff_hz: f64,
    gain_db: f64,
    bypass: bool,
    sample_rate: f64,
    hz: f64,
) -> f64 {
    if bypass {
        gain_db
    } else {
        let stage = make_lr4_stage(cutoff_hz, sample_rate);
        2.0 * stage.gain_db_at(hz) + gain_db
    }
}

/// C1-continuous soft ceiling function.
/// Linear identity up to +/-0.92, with continuous slope = 1.0 at the knee
/// and asymptotic compression strictly inside (-1.0, 1.0).
#[inline]
pub fn soft_ceiling(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    const T: f32 = 0.92;
    const CEIL: f32 = 0.9999;
    let abs_x = x.abs();
    if abs_x <= T {
        x
    } else {
        let headroom = CEIL - T;
        let excess = (abs_x - T) / headroom;
        x.signum() * (T + headroom * excess.tanh())
    }
}

/// Clean envelope-following stereo-linked peak limiter (fast attack, smooth release).
/// Prevents overshoot without generating static waveshaping harmonics at normal levels.
#[derive(Clone, Copy, Debug)]
pub struct PeakLimiter {
    pub env_gain: f32,
    pub attack_coeff: f32,
    pub release_coeff: f32,
    pub ceiling: f32,
}

impl PeakLimiter {
    pub fn new(sample_rate: f64) -> Self {
        let sr = sample_rate.max(1.0);
        let attack_coeff = (1.0 - (-1.0 / (0.0005 * sr)).exp()) as f32;
        let release_coeff = (1.0 - (-1.0 / (0.080 * sr)).exp()) as f32;
        Self {
            env_gain: 1.0,
            attack_coeff,
            release_coeff,
            ceiling: 0.92,
        }
    }

    pub fn reset(&mut self) {
        self.env_gain = 1.0;
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs());
        let target = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        let coeff = if target < self.env_gain {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.env_gain += coeff * (target - self.env_gain);
        (
            soft_ceiling(l * self.env_gain),
            soft_ceiling(r * self.env_gain),
        )
    }
}

/// Linkwitz-Riley 4th-order (24 dB/octave) low-pass processor with lock-free-friendly delay line,
/// smoothed gain trim, phase inversion, bypass, and dynamic peak limiter.
#[derive(Clone, Debug)]
pub struct SubwooferDsp {
    pub sample_rate: f64,
    pub cutoff_hz: f64,
    pub gain_db: f32,
    pub gain_target_linear: f32,
    pub gain_current_linear: f32,
    pub gain_smooth_coeff: f32,
    pub stereo: bool,
    pub phase_inverted: bool,
    pub bypass: bool,
    pub limiter: bool,
    pub direct_sum: bool,
    pub delay_samples: usize,
    pub delay_buffer_l: Vec<f32>,
    pub delay_buffer_r: Vec<f32>,
    pub delay_write_pos: usize,
    pub stage1_l: Biquad,
    pub stage2_l: Biquad,
    pub stage1_r: Biquad,
    pub stage2_r: Biquad,
    pub peak_limiter: PeakLimiter,
}

impl SubwooferDsp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cutoff_hz: f32,
        gain_db: f32,
        delay_ms: f32,
        stereo: bool,
        phase_inverted: bool,
        bypass: bool,
        limiter: bool,
        direct_sum: bool,
        sample_rate: u32,
    ) -> Self {
        let sample_rate_f = f64::from(sample_rate.max(1));
        let cutoff = f64::from(cutoff_hz).clamp(20.0, 1000.0);
        let q = std::f64::consts::FRAC_1_SQRT_2;
        let s1_l = Biquad::lowpass(cutoff, q, sample_rate_f);
        let s2_l = Biquad::lowpass(cutoff, q, sample_rate_f);
        let s1_r = Biquad::lowpass(cutoff, q, sample_rate_f);
        let s2_r = Biquad::lowpass(cutoff, q, sample_rate_f);

        let max_delay_cap = ((0.5 * sample_rate_f).round() as usize) + 2;
        let delay_samples = ((delay_ms.clamp(0.0, 500.0) as f64 * sample_rate_f / 1000.0).round()
            as usize)
            .min(max_delay_cap.saturating_sub(1));

        let gain_db = gain_db.clamp(-30.0, 36.0);
        let gain_linear = 10.0f32.powf(gain_db / 20.0);
        let gain_smooth_coeff = (1.0 - (-1.0 / (0.005 * sample_rate_f)).exp()) as f32;

        Self {
            sample_rate: sample_rate_f,
            cutoff_hz: cutoff,
            gain_db,
            gain_target_linear: gain_linear,
            gain_current_linear: gain_linear,
            gain_smooth_coeff,
            stereo,
            phase_inverted,
            bypass,
            limiter,
            direct_sum,
            delay_samples,
            delay_buffer_l: vec![0.0f32; max_delay_cap],
            delay_buffer_r: vec![0.0f32; max_delay_cap],
            delay_write_pos: 0,
            stage1_l: s1_l,
            stage2_l: s2_l,
            stage1_r: s1_r,
            stage2_r: s2_r,
            peak_limiter: PeakLimiter::new(sample_rate_f),
        }
    }

    pub fn reset(&mut self) {
        self.stage1_l.reset();
        self.stage2_l.reset();
        self.stage1_r.reset();
        self.stage2_r.reset();
        self.delay_buffer_l.fill(0.0);
        self.delay_buffer_r.fill(0.0);
        self.delay_write_pos = 0;
        self.gain_current_linear = self.gain_target_linear;
        self.peak_limiter.reset();
    }

    pub fn set_cutoff(&mut self, cutoff_hz: f32) {
        let cutoff = f64::from(cutoff_hz).clamp(20.0, 1000.0);
        if (cutoff - self.cutoff_hz).abs() > 0.01 {
            self.cutoff_hz = cutoff;
            let q = std::f64::consts::FRAC_1_SQRT_2;
            let s1 = Biquad::lowpass(cutoff, q, self.sample_rate);
            let s2 = Biquad::lowpass(cutoff, q, self.sample_rate);
            self.stage1_l.b0 = s1.b0;
            self.stage1_l.b1 = s1.b1;
            self.stage1_l.b2 = s1.b2;
            self.stage1_l.a1 = s1.a1;
            self.stage1_l.a2 = s1.a2;
            self.stage2_l.b0 = s2.b0;
            self.stage2_l.b1 = s2.b1;
            self.stage2_l.b2 = s2.b2;
            self.stage2_l.a1 = s2.a1;
            self.stage2_l.a2 = s2.a2;
            self.stage1_r.b0 = s1.b0;
            self.stage1_r.b1 = s1.b1;
            self.stage1_r.b2 = s1.b2;
            self.stage1_r.a1 = s1.a1;
            self.stage1_r.a2 = s1.a2;
            self.stage2_r.b0 = s2.b0;
            self.stage2_r.b1 = s2.b1;
            self.stage2_r.b2 = s2.b2;
            self.stage2_r.a1 = s2.a1;
            self.stage2_r.a2 = s2.a2;
        }
    }

    pub fn set_gain_db(&mut self, gain_db: f32) {
        let clamped = gain_db.clamp(-30.0, 36.0);
        if (clamped - self.gain_db).abs() > 0.001 {
            self.gain_db = clamped;
            self.gain_target_linear = 10.0f32.powf(clamped / 20.0);
        }
    }

    pub fn set_delay_ms(&mut self, delay_ms: f32) {
        let cap = self.delay_buffer_l.len();
        let new_samples = ((delay_ms.clamp(0.0, 500.0) as f64 * self.sample_rate / 1000.0).round()
            as usize)
            .min(cap.saturating_sub(1));
        self.delay_samples = new_samples;
    }

    pub fn set_stereo(&mut self, stereo: bool) {
        if self.stereo != stereo {
            self.stereo = stereo;
            self.stage1_r.reset();
            self.stage2_r.reset();
        }
    }

    pub fn set_phase_inverted(&mut self, phase_inverted: bool) {
        self.phase_inverted = phase_inverted;
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    pub fn set_limiter(&mut self, limiter: bool) {
        if self.limiter != limiter {
            self.limiter = limiter;
            if !limiter {
                self.peak_limiter.reset();
            }
        }
    }

    pub fn set_direct_sum(&mut self, direct_sum: bool) {
        self.direct_sum = direct_sum;
    }

    /// Evaluates the total magnitude response of the LR4 filter at `hz`.
    pub fn lr4_gain_db_at(&self, hz: f64) -> f64 {
        if self.bypass {
            self.gain_db as f64
        } else {
            self.stage1_l.gain_db_at(hz) + self.stage2_l.gain_db_at(hz) + self.gain_db as f64
        }
    }

    #[inline]
    pub fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.gain_current_linear +=
            self.gain_smooth_coeff * (self.gain_target_linear - self.gain_current_linear);
        let gain = self.gain_current_linear;
        let sum_mult = if self.direct_sum { 1.0f32 } else { 0.5f32 };

        let (raw_l, raw_r) = if self.stereo {
            (frame[0] * gain, frame[1] * gain)
        } else {
            let mono = (frame[0] + frame[1]) * sum_mult * gain;
            (mono, mono)
        };

        let (filtered_l, filtered_r) = if self.bypass {
            (raw_l, raw_r)
        } else if self.stereo {
            let s1_l = self.stage1_l.run(raw_l as f64);
            let s2_l = self.stage2_l.run(s1_l) as f32;
            let s1_r = self.stage1_r.run(raw_r as f64);
            let s2_r = self.stage2_r.run(s1_r) as f32;
            (s2_l, s2_r)
        } else {
            let s1 = self.stage1_l.run(raw_l as f64);
            let s2 = self.stage2_l.run(s1) as f32;
            (s2, s2)
        };

        let (del_l, del_r) = if self.delay_samples > 0 {
            let cap = self.delay_buffer_l.len();
            self.delay_buffer_l[self.delay_write_pos] = filtered_l;
            self.delay_buffer_r[self.delay_write_pos] = filtered_r;
            let read_pos = (self.delay_write_pos + cap - self.delay_samples) % cap;
            let out_l = self.delay_buffer_l[read_pos];
            let out_r = self.delay_buffer_r[read_pos];
            self.delay_write_pos = (self.delay_write_pos + 1) % cap;
            (out_l, out_r)
        } else {
            (filtered_l, filtered_r)
        };

        let phase_mult = if self.phase_inverted { -1.0f32 } else { 1.0f32 };
        let mut out_l = del_l * phase_mult;
        let mut out_r = del_r * phase_mult;

        if self.limiter {
            (out_l, out_r) = self.peak_limiter.process(out_l, out_r);
        }

        [out_l, out_r]
    }

    /// Process interleaved stereo audio samples in-place.
    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(2) {
            let out = self.process_frame([frame[0], frame[1]]);
            frame[0] = out[0];
            frame[1] = out[1];
        }
    }
}

/// Adaptive cubic (Catmull-Rom) stereo resampler with optional anti-aliasing filter
/// when downsampling, supporting smooth clock-drift adjustment across WASAPI streams.
pub struct AdaptiveResampler {
    base_ratio: f64,
    phase: f64,
    w: [[f32; 2]; 4],
    aa_enabled: bool,
    aa_l: Biquad,
    aa_r: Biquad,
}

impl AdaptiveResampler {
    pub fn new(from_rate: u32, to_rate: u32) -> Self {
        let from_f = from_rate.max(1) as f64;
        let to_f = to_rate.max(1) as f64;
        let aa_enabled = from_rate > to_rate.saturating_add(100);
        let aa_fc = (0.45 * to_f).clamp(100.0, from_f * 0.49);
        let q = std::f64::consts::FRAC_1_SQRT_2;
        Self {
            base_ratio: from_f / to_f,
            phase: 1.0,
            w: [[0.0; 2]; 4],
            aa_enabled,
            aa_l: Biquad::lowpass(aa_fc, q, from_f),
            aa_r: Biquad::lowpass(aa_fc, q, from_f),
        }
    }

    pub fn base_ratio(&self) -> f64 {
        self.base_ratio
    }

    pub fn reset(&mut self) {
        self.w = [[0.0; 2]; 4];
        self.phase = 1.0;
        self.aa_l.reset();
        self.aa_r.reset();
    }

    pub fn next_frame(
        &mut self,
        fifo: &mut VecDeque<[f32; 2]>,
        step_ratio: f64,
    ) -> Option<[f32; 2]> {
        while self.phase >= 1.0 {
            let mut s = fifo.pop_front()?;
            if self.aa_enabled {
                s[0] = self.aa_l.run(s[0] as f64) as f32;
                s[1] = self.aa_r.run(s[1] as f64) as f32;
            }
            self.w[0] = self.w[1];
            self.w[1] = self.w[2];
            self.w[2] = self.w[3];
            self.w[3] = s;
            self.phase -= 1.0;
        }

        let t = self.phase as f32;
        let out = if t <= 1e-7 {
            self.w[1]
        } else {
            [
                catmull_rom(self.w[0][0], self.w[1][0], self.w[2][0], self.w[3][0], t),
                catmull_rom(self.w[0][1], self.w[1][1], self.w[2][1], self.w[3][1], t),
            ]
        };

        self.phase += step_ratio;
        Some(out)
    }
}

#[inline]
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Adaptive trough-tracking jitter buffer controller that keeps capture-to-subwoofer
/// queue latency minimal (3 ms default trough) while absorbing mismatched WASAPI periods.
pub struct JitterController {
    in_rate: u32,
    out_rate: u32,
    base_ratio: f64,
    pub cushion_ms: f64,
    primed: bool,
    ramp: f32,
    err_smooth: f64,
    step_ratio: f64,
    pub silent_callbacks: u32,
    post_window: [usize; 8],
    post_window_idx: usize,
    post_window_count: usize,
    steady_cb_out: usize,
    packet_frames: usize,
    last_gap_frames: usize,
}

impl JitterController {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let in_r = in_rate.max(1);
        let out_r = out_rate.max(1);
        let base_ratio = in_r as f64 / out_r as f64;
        Self {
            in_rate: in_r,
            out_rate: out_r,
            base_ratio,
            cushion_ms: 3.0,
            primed: false,
            ramp: 0.0,
            err_smooth: 0.0,
            step_ratio: base_ratio,
            silent_callbacks: 0,
            post_window: [0; 8],
            post_window_idx: 0,
            post_window_count: 0,
            steady_cb_out: 0,
            packet_frames: 0,
            last_gap_frames: 0,
        }
    }

    pub fn cushion_frames(&self) -> usize {
        ((self.in_rate as f64 * self.cushion_ms) / 1000.0)
            .round()
            .max(64.0) as usize
    }

    pub fn begin_callback(
        &mut self,
        fifo: &mut VecDeque<[f32; 2]>,
        received_frames: usize,
        needed_out_frames: usize,
    ) -> f64 {
        if received_frames > 0 {
            self.silent_callbacks = 0;
        } else {
            self.silent_callbacks = self.silent_callbacks.saturating_add(1);
        }

        let max_single_packet = (self.in_rate as usize * 12) / 1000;
        if received_frames > 0 && received_frames <= max_single_packet {
            self.packet_frames = self.packet_frames.max(received_frames);
        }

        let min_cb = ((self.out_rate as usize * 2) / 1000).max(64);
        let bounded_out = needed_out_frames.max(min_cb);
        if self.steady_cb_out == 0 {
            self.steady_cb_out = bounded_out;
        } else {
            self.steady_cb_out = self.steady_cb_out.min(bounded_out);
        }

        let fractional_margin = usize::from((self.base_ratio - 1.0).abs() > 1e-4);
        let needed_in = (needed_out_frames as f64 * self.base_ratio).ceil() as usize + fractional_margin;
        let gap_frames = self.packet_frames.saturating_sub(needed_in);
        self.last_gap_frames = gap_frames;
        let prime_target = needed_in + self.cushion_frames() + gap_frames;

        if !self.primed && fifo.len() >= prime_target {
            if fifo.len() > prime_target {
                let trim = fifo.len() - prime_target;
                fifo.drain(..trim);
            }
            self.primed = true;
            self.ramp = 0.0;
            self.err_smooth = 0.0;
            self.step_ratio = self.base_ratio;
            self.post_window_count = 0;
            self.post_window_idx = 0;
        }

        self.step_ratio
    }

    pub fn is_primed(&self) -> bool {
        self.primed
    }

    #[inline]
    pub fn next_fade_gain(&mut self) -> f32 {
        self.ramp = (self.ramp + (1.0 / 48.0)).min(1.0);
        self.ramp
    }

    pub fn on_underrun(&mut self) {
        self.primed = false;
        self.ramp = 0.0;
        self.err_smooth = 0.0;
        self.step_ratio = self.base_ratio;
        self.post_window_count = 0;
    }

    pub fn end_callback(
        &mut self,
        fifo: &mut VecDeque<[f32; 2]>,
        needed_out_frames: usize,
        underran: bool,
    ) {
        if underran {
            if self.silent_callbacks <= 4 {
                self.cushion_ms = (self.cushion_ms + 2.0).min(14.0);
            }
            return;
        }
        if !self.primed {
            return;
        }

        let cushion = self.cushion_frames();
        let needed_in = (needed_out_frames as f64 * self.base_ratio).ceil() as usize;
        let target_trough = cushion;
        let keep_post = cushion + self.last_gap_frames;
        let max_post = keep_post + ((self.in_rate as usize * 25) / 1000).max(needed_in);

        if fifo.len() > max_post {
            fifo.drain(..fifo.len() - keep_post);
            self.err_smooth = 0.0;
            self.step_ratio = self.base_ratio;
            self.post_window_count = 0;
            return;
        }

        self.post_window[self.post_window_idx] = fifo.len();
        self.post_window_idx = (self.post_window_idx + 1) % self.post_window.len();
        self.post_window_count = (self.post_window_count + 1).min(self.post_window.len());

        let min_post = self.post_window[..self.post_window_count]
            .iter()
            .copied()
            .min()
            .unwrap_or(target_trough);

        let fill_err = (min_post as f64 - target_trough as f64) / (target_trough as f64).max(128.0);
        self.err_smooth += 0.05 * (fill_err - self.err_smooth);
        let trim = (self.err_smooth * 0.004).clamp(-0.008, 0.008);
        self.step_ratio = self.base_ratio * (1.0 + trim);
    }

    pub fn estimated_buffer_ms(&self, post_fill: usize, needed_out_frames: usize) -> f32 {
        let cb_out = if self.steady_cb_out > 0 {
            self.steady_cb_out
        } else {
            needed_out_frames
        };
        let fifo_ms = post_fill as f32 * 1000.0 / self.in_rate as f32;
        let out_ms = cb_out as f32 * 1000.0 / self.out_rate as f32;
        fifo_ms + out_ms
    }
}

/// Computes a Hann-windowed radix-2 Cooley-Tukey FFT magnitude spectrum in dBFS.
/// Returns `FFT_SIZE / 2` bins.
pub fn compute_spectrum_db(samples: &[f32], out_bins: &mut Vec<f32>) {
    let n = FFT_SIZE;
    let half = n / 2;
    out_bins.clear();
    out_bins.resize(half, -120.0);

    if samples.is_empty() {
        return;
    }

    let mut re = vec![0.0f32; n];
    let mut im = vec![0.0f32; n];

    let start = samples.len().saturating_sub(n);
    let slice = &samples[start..];
    let offset = n - slice.len();
    let inv_n = 1.0 / n as f32;
    for (i, &s) in slice.iter().enumerate() {
        let idx = offset + i;
        let w = 0.5 * (1.0 - (std::f32::consts::TAU * idx as f32 * inv_n).cos());
        re[idx] = s * w;
    }

    // Bit-reversal permutation
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
        }
    }

    // Iterative Cooley-Tukey butterflies
    let mut len = 2usize;
    while len <= n {
        let half_len = len / 2;
        let angle_step = -std::f64::consts::TAU / len as f64;
        let w_step_re = angle_step.cos();
        let w_step_im = angle_step.sin();

        let mut i = 0;
        while i < n {
            let mut w_re = 1.0f64;
            let mut w_im = 0.0f64;
            for k in 0..half_len {
                let wr = w_re as f32;
                let wi = w_im as f32;
                let u_idx = i + k;
                let v_idx = u_idx + half_len;
                let vr = re[v_idx] * wr - im[v_idx] * wi;
                let vi = re[v_idx] * wi + im[v_idx] * wr;
                re[v_idx] = re[u_idx] - vr;
                im[v_idx] = im[u_idx] - vi;
                re[u_idx] += vr;
                im[u_idx] += vi;

                let next_w_re = w_re * w_step_re - w_im * w_step_im;
                w_im = w_re * w_step_im + w_im * w_step_re;
                w_re = next_w_re;
            }
            i += len;
        }
        len <<= 1;
    }

    let scale = 4.0 / n as f32;
    for (k, out) in out_bins.iter_mut().enumerate() {
        let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() * scale;
        *out = 20.0 * mag.max(1e-6).log10();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biquad_butterworth_lowpass_response() {
        let lpf = Biquad::lowpass(80.0, std::f64::consts::FRAC_1_SQRT_2, 44100.0);

        let gain_20 = lpf.gain_db_at(20.0);
        assert!(
            gain_20.abs() < 0.1,
            "Passband at 20 Hz should be ~0 dB, got {gain_20}"
        );

        let gain_80 = lpf.gain_db_at(80.0);
        assert!(
            (gain_80 - (-3.01)).abs() < 0.1,
            "Cutoff at 80 Hz should be ~ -3.01 dB, got {gain_80}"
        );

        let gain_160 = lpf.gain_db_at(160.0);
        assert!(
            (gain_160 - (-12.3)).abs() < 0.5,
            "1 octave above cutoff should be ~ -12.3 dB, got {gain_160}"
        );
    }

    #[test]
    fn subwoofer_dsp_lr4_crossover_response() {
        let dsp = SubwooferDsp::new(80.0, 0.0, 0.0, false, false, false, false, false, 44100);

        let gain_20 = dsp.stage1_l.gain_db_at(20.0) + dsp.stage2_l.gain_db_at(20.0);
        assert!(
            gain_20.abs() < 0.2,
            "20 Hz sub-bass should be ~0 dB, got {gain_20}"
        );

        let gain_80 = dsp.stage1_l.gain_db_at(80.0) + dsp.stage2_l.gain_db_at(80.0);
        assert!(
            (gain_80 - (-6.02)).abs() < 0.25,
            "LR4 crossover at 80 Hz must be -6.02 dB, got {gain_80}"
        );

        let gain_160 = dsp.stage1_l.gain_db_at(160.0) + dsp.stage2_l.gain_db_at(160.0);
        assert!(
            (gain_160 - (-24.6)).abs() < 1.0,
            "LR4 rolloff 1 octave above cutoff must be ~ -24 dB/oct, got {gain_160}"
        );
    }

    #[test]
    fn subwoofer_dsp_mono_downmix() {
        let mut dsp = SubwooferDsp::new(80.0, 0.0, 0.0, false, false, false, false, false, 44100);
        let mut samples = vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        dsp.process(&mut samples);
        assert_eq!(samples[0], samples[1]);
        assert_eq!(samples[2], samples[3]);
        assert_eq!(samples[4], samples[5]);
    }

    #[test]
    fn subwoofer_dsp_stereo_separation() {
        let mut dsp = SubwooferDsp::new(80.0, 0.0, 0.0, true, false, false, false, false, 44100);
        let mut samples = vec![1.0, 0.0, 1.0, 0.0];
        dsp.process(&mut samples);
        assert!(samples[0] > 0.0);
        assert_eq!(samples[1], 0.0);
    }

    #[test]
    fn subwoofer_dsp_delay_line() {
        let mut dsp = SubwooferDsp::new(80.0, 0.0, 1.0, true, false, true, false, false, 44100);
        assert_eq!(dsp.delay_samples, 44);

        let mut pulse = vec![0.0f32; 92];
        pulse[0] = 0.75;
        pulse[1] = -0.5;
        dsp.process(&mut pulse);

        // First 44 frames must still output the initial zero buffer
        for i in 0..88 {
            assert_eq!(pulse[i], 0.0, "Sample {i} should still be zero before 44-frame delay");
        }
        // Frame 44 (indices 88, 89) must yield the impulse from frame 0
        assert!((pulse[88] - 0.75).abs() < 1e-6);
        assert!((pulse[89] - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn subwoofer_dsp_phase_inversion() {
        let mut dsp_norm =
            SubwooferDsp::new(80.0, 0.0, 0.0, true, false, false, false, false, 44100);
        let mut dsp_inv = SubwooferDsp::new(80.0, 0.0, 0.0, true, true, false, false, false, 44100);

        let mut s_norm = vec![0.5, -0.5, 0.8, -0.2];
        let mut s_inv = s_norm.clone();

        dsp_norm.process(&mut s_norm);
        dsp_inv.process(&mut s_inv);

        for (a, b) in s_norm.iter().zip(s_inv.iter()) {
            assert!((a + b).abs() < 1e-6, "Inverted phase should negate signal");
        }
    }

    #[test]
    fn subwoofer_dsp_bypass() {
        let mut dsp = SubwooferDsp::new(80.0, 0.0, 0.0, true, false, true, false, false, 44100);
        let original = vec![0.5, -0.25, 0.8, -0.1];
        let mut samples = original.clone();
        dsp.process(&mut samples);
        for (a, b) in original.iter().zip(samples.iter()) {
            assert!(
                (a - b).abs() < 1e-6,
                "Bypass must leave samples un-filtered"
            );
        }
    }

    #[test]
    fn soft_ceiling_c1_continuity_and_bounds() {
        assert_eq!(soft_ceiling(0.5), 0.5);
        assert_eq!(soft_ceiling(-0.92), -0.92);

        let below = soft_ceiling(0.92 - 1e-4);
        let at = soft_ceiling(0.92);
        let above = soft_ceiling(0.92 + 1e-4);
        assert!((at - below).abs() < 2e-4);
        assert!((above - at).abs() < 2e-4);

        let mut prev = -1.0;
        for step in -200..=200 {
            let x = step as f32 * 0.05;
            let y = soft_ceiling(x);
            assert!(y > -1.0 && y < 1.0, "Output {y} out of strict (-1, 1) bounds at x={x}");
            assert!(y >= prev, "Must remain monotonic at x={x}");
            prev = y;
        }
    }

    #[test]
    fn peak_limiter_controls_hot_sine() {
        let mut limiter = PeakLimiter::new(48000.0);
        let mut max_steady = 0.0f32;
        let mut flat_top_count = 0usize;
        let mut prev = 0.0f32;

        for n in 0..4800 {
            let s = 4.0 * (2.0 * std::f32::consts::PI * 60.0 * n as f32 / 48000.0).sin();
            let (out_l, out_r) = limiter.process(s, s);
            assert!((-1.0..=1.0).contains(&out_l), "Sample {n} exceeded bounds: {out_l}");
            assert_eq!(out_l, out_r);
            if n >= 2400 {
                max_steady = max_steady.max(out_l.abs());
                if (out_l - prev).abs() < 1e-7 && out_l.abs() > 0.99 {
                    flat_top_count += 1;
                }
            }
            prev = out_l;
        }

        assert!(max_steady <= 0.9999, "Steady peak too high: {max_steady}");
        assert!(max_steady > 0.5, "Limiter collapsed signal: {max_steady}");
        assert_eq!(flat_top_count, 0, "Detected flat-top hard clipping");
    }

    #[test]
    fn adaptive_resampler_identity_and_conversion() {
        // 1:1 rate preserves frames exactly after 2-frame Catmull-Rom lookahead
        let mut id = AdaptiveResampler::new(48000, 48000);
        let mut fifo = VecDeque::new();
        for i in 1..=10 {
            fifo.push_back([i as f32 * 0.1, -(i as f32 * 0.1)]);
        }
        let mut pulled = Vec::new();
        while let Some(f) = id.next_frame(&mut fifo, id.base_ratio()) {
            pulled.push(f);
        }
        assert_eq!(pulled.len(), 10);
        for i in 0..8 {
            let expected = (i + 1) as f32 * 0.1;
            assert!((pulled[i + 2][0] - expected).abs() < 1e-6);
            assert!((pulled[i + 2][1] + expected).abs() < 1e-6);
        }

        // Cross-rate 44100 -> 48000 produces smooth sine frames without losing leftovers
        let mut up = AdaptiveResampler::new(44100, 48000);
        let mut up_fifo = VecDeque::new();
        for n in 0..4410 {
            let v = (2.0 * std::f32::consts::PI * 50.0 * n as f32 / 44100.0).sin();
            up_fifo.push_back([v, v]);
        }
        let mut up_out = Vec::new();
        while let Some(f) = up.next_frame(&mut up_fifo, up.base_ratio()) {
            up_out.push(f);
        }
        // ~4800 output frames from 4410 input frames
        assert!((up_out.len() as isize - 4800).abs() <= 4, "Got {}", up_out.len());
    }

    #[test]
    fn fft_spectrum_detects_80hz_tone() {
        let mut samples = vec![0.0f32; FFT_SIZE];
        for (n, s) in samples.iter_mut().enumerate() {
            *s = (2.0 * std::f32::consts::PI * 80.0 * n as f32 / 48000.0).sin();
        }
        let mut bins = Vec::new();
        compute_spectrum_db(&samples, &mut bins);
        assert_eq!(bins.len(), FFT_SIZE / 2);

        let (peak_bin, &peak_db) = bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();

        assert!(
            (6..=8).contains(&peak_bin),
            "Expected peak bin in 6..=8 for 80 Hz at 48 kHz, got bin {peak_bin}"
        );
        assert!(
            (-3.0..=1.0).contains(&peak_db),
            "Expected peak magnitude around 0 dB, got {peak_db} dB"
        );

        let bin_500 = ((500.0 * FFT_SIZE as f32) / 48000.0).round() as usize;
        assert!(
            bins[bin_500] < -40.0,
            "Expected 500 Hz noise floor < -40 dB, got {} dB",
            bins[bin_500]
        );
    }

    #[test]
    fn jitter_controller_trims_startup_backlog() {
        let mut jc = JitterController::new(48000, 48000);
        let mut fifo = VecDeque::new();
        for i in 0..2400 {
            fifo.push_back([i as f32 * 0.001, i as f32 * 0.001]);
        }

        let ratio = jc.begin_callback(&mut fifo, 2400, 480);
        assert!((ratio - 1.0).abs() < 1e-9);
        assert!(jc.is_primed());
        // Must trim 50 ms backlog down to needed_in (480) + cushion (144 = 3 ms)
        assert_eq!(fifo.len(), 624);
        // Kept newest tail rather than stale startup audio
        assert!((fifo.front().unwrap()[0] - (2400 - 624) as f32 * 0.001).abs() < 1e-6);
    }

    #[test]
    fn jitter_controller_mismatched_periods_stays_glitch_free() {
        let mut jc = JitterController::new(48000, 48000);
        let mut resampler = AdaptiveResampler::new(48000, 48000);
        let mut fifo = VecDeque::new();
        let mut underruns_after_prime = 0usize;
        let mut was_primed = false;
        let mut min_post_after_settle = usize::MAX;

        for cb in 0..200 {
            let received = if cb % 2 == 0 {
                for s in 0..480 {
                    let v = (cb * 480 + s) as f32 * 0.0001;
                    fifo.push_back([v, v]);
                }
                480
            } else {
                0
            };

            let step = jc.begin_callback(&mut fifo, received, 240);
            let mut underran = false;

            if jc.is_primed() {
                was_primed = true;
                for _ in 0..240 {
                    if resampler.next_frame(&mut fifo, step).is_some() {
                        let _ = jc.next_fade_gain();
                    } else {
                        underran = true;
                        jc.on_underrun();
                        resampler.reset();
                        break;
                    }
                }
            }

            if was_primed && underran {
                underruns_after_prime += 1;
            }
            jc.end_callback(&mut fifo, 240, underran);

            if cb >= 10 && jc.is_primed() {
                min_post_after_settle = min_post_after_settle.min(fifo.len());
            }
        }

        assert!(was_primed);
        assert_eq!(underruns_after_prime, 0, "Expected zero underruns after priming");
        // Trough should stay near 3 ms (144 frames <= 288 = 6 ms)
        assert!(
            min_post_after_settle <= 288 && min_post_after_settle >= 64,
            "Unexpected steady trough: {min_post_after_settle} frames"
        );
    }

    #[test]
    fn jitter_controller_expands_only_on_active_underrun() {
        let mut jc = JitterController::new(48000, 48000);
        let mut fifo = VecDeque::new();

        // Underrun right after receiving capture audio (real mid-playback jitter)
        jc.begin_callback(&mut fifo, 480, 480);
        fifo.clear();
        jc.end_callback(&mut fifo, 480, true);
        assert!((jc.cushion_ms - 5.0).abs() < 1e-9);

        // Underrun after 5 silent callbacks (WASAPI idle pause) must NOT inflate cushion
        for _ in 0..5 {
            jc.begin_callback(&mut fifo, 0, 480);
        }
        jc.end_callback(&mut fifo, 480, true);
        assert!((jc.cushion_ms - 5.0).abs() < 1e-9);
    }
}
