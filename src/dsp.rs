use std::f64::consts::PI;

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
    /// 2nd-order Butterworth low-pass filter with cutoff frequency `fc` and Q factor.
    pub fn lowpass(fc: f64, q: f64, sample_rate: f64) -> Self {
        let nyquist = sample_rate * 0.5;
        let fc = fc.clamp(1.0, nyquist - 1.0);
        let omega = 2.0 * PI * fc / sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * q);

        let a0 = 1.0 + alpha;
        let b0 = ((1.0 - cos_omega) * 0.5) / a0;
        let b1 = (1.0 - cos_omega) / a0;
        let b2 = ((1.0 - cos_omega) * 0.5) / a0;
        let a1 = (-2.0 * cos_omega) / a0;
        let a2 = (1.0 - alpha) / a0;

        Self {
            sample_rate,
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

    /// Evaluates filter gain in decibels at `hz` using the discrete transfer function H(e^{j\omega}).
    pub fn gain_db_at(&self, hz: f64) -> f64 {
        let w = std::f64::consts::TAU * hz / self.sample_rate;
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let numerator = (self.b0 + self.b1 * cos1 + self.b2 * cos2).powi(2)
            + (self.b1 * sin1 + self.b2 * sin2).powi(2);
        let denominator = (1.0 + self.a1 * cos1 + self.a2 * cos2).powi(2)
            + (self.a1 * sin1 + self.a2 * sin2).powi(2);
        10.0 * (numerator / denominator).log10()
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

/// Linkwitz-Riley 4th-order (24 dB/octave) Low-Pass Filter with delay, gain, phase inversion & bypass.
#[derive(Clone, Debug)]
pub struct SubwooferDsp {
    pub sample_rate: f64,
    pub cutoff_hz: f64,
    pub gain_db: f32,
    pub gain_linear: f32,
    pub stereo: bool,
    pub phase_inverted: bool,
    pub bypass: bool,
    pub soft_clip: bool,
    pub direct_sum: bool,
    pub delay_samples: usize,
    pub delay_buffer_l: Vec<f32>,
    pub delay_buffer_r: Vec<f32>,
    pub delay_write_pos: usize,
    pub stage1_l: Biquad,
    pub stage2_l: Biquad,
    pub stage1_r: Biquad,
    pub stage2_r: Biquad,
}

impl SubwooferDsp {
    pub fn new(
        cutoff_hz: f32,
        gain_db: f32,
        delay_ms: f32,
        stereo: bool,
        phase_inverted: bool,
        bypass: bool,
        soft_clip: bool,
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
        let delay_samples = (delay_ms.clamp(0.0, 500.0) as f64 * sample_rate_f / 1000.0).round() as usize;
        let delay_buffer_l = if delay_samples > 0 {
            vec![0.0f32; delay_samples]
        } else {
            Vec::new()
        };
        let delay_buffer_r = if delay_samples > 0 {
            vec![0.0f32; delay_samples]
        } else {
            Vec::new()
        };
        let gain_db = gain_db.clamp(-30.0, 36.0);
        let gain_linear = 10.0f32.powf(gain_db / 20.0);
        Self {
            sample_rate: sample_rate_f,
            cutoff_hz: cutoff,
            gain_db,
            gain_linear,
            stereo,
            phase_inverted,
            bypass,
            soft_clip,
            direct_sum,
            delay_samples,
            delay_buffer_l,
            delay_buffer_r,
            delay_write_pos: 0,
            stage1_l: s1_l,
            stage2_l: s2_l,
            stage1_r: s1_r,
            stage2_r: s2_r,
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
        self.gain_db = gain_db.clamp(-30.0, 36.0);
        self.gain_linear = 10.0f32.powf(self.gain_db / 20.0);
    }

    pub fn set_delay_ms(&mut self, delay_ms: f32) {
        let new_samples = (delay_ms.clamp(0.0, 500.0) as f64 * self.sample_rate / 1000.0).round() as usize;
        if new_samples != self.delay_samples {
            self.delay_samples = new_samples;
            self.delay_buffer_l = if new_samples > 0 {
                vec![0.0f32; new_samples]
            } else {
                Vec::new()
            };
            self.delay_buffer_r = if new_samples > 0 {
                vec![0.0f32; new_samples]
            } else {
                Vec::new()
            };
            self.delay_write_pos = 0;
        }
    }

    pub fn set_stereo(&mut self, stereo: bool) {
        if self.stereo != stereo {
            self.stereo = stereo;
            self.reset();
        }
    }

    pub fn set_phase_inverted(&mut self, phase_inverted: bool) {
        self.phase_inverted = phase_inverted;
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    pub fn set_soft_clip(&mut self, soft_clip: bool) {
        self.soft_clip = soft_clip;
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

    /// Process interleaved stereo audio samples in-place.
    pub fn process(&mut self, samples: &mut [f32]) {
        let has_delay = !self.delay_buffer_l.is_empty();
        let phase_mult = if self.phase_inverted { -1.0f32 } else { 1.0f32 };
        let sum_mult = if self.direct_sum { 1.0f32 } else { 0.5f32 };

        for frame in samples.chunks_exact_mut(2) {
            let (raw_l, raw_r) = if self.stereo {
                (frame[0] * self.gain_linear, frame[1] * self.gain_linear)
            } else {
                let mono = (frame[0] + frame[1]) * sum_mult * self.gain_linear;
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

            let (out_l, out_r) = if has_delay {
                let del_l = self.delay_buffer_l[self.delay_write_pos];
                let del_r = self.delay_buffer_r[self.delay_write_pos];
                self.delay_buffer_l[self.delay_write_pos] = filtered_l;
                self.delay_buffer_r[self.delay_write_pos] = filtered_r;
                (del_l, del_r)
            } else {
                (filtered_l, filtered_r)
            };

            let mut final_l = out_l * phase_mult;
            let mut final_r = out_r * phase_mult;

            if self.soft_clip {
                final_l = soft_saturate(final_l);
                final_r = soft_saturate(final_r);
            }

            frame[0] = final_l;
            frame[1] = final_r;

            if has_delay {
                self.delay_write_pos = (self.delay_write_pos + 1) % self.delay_samples;
            }
        }
    }
}

/// Analog-modeled hyperbolic tangent soft saturation to prevent harsh digital clipping.
#[inline]
pub fn soft_saturate(x: f32) -> f32 {
    if x.abs() < 0.65 {
        x
    } else {
        x.tanh()
    }
}

/// Simple, ultra-low latency stereo resampler (linear interpolation).
pub struct Resampler {
    ratio: f64,
    phase: f64,
    last_frame: [f32; 2],
}

impl Resampler {
    pub fn new(from_rate: u32, to_rate: u32) -> Self {
        Self {
            ratio: from_rate as f64 / to_rate as f64,
            phase: 0.0,
            last_frame: [0.0, 0.0],
        }
    }

    pub fn resample(&mut self, input: &[[f32; 2]], output: &mut Vec<[f32; 2]>) {
        if (self.ratio - 1.0).abs() < 1e-4 {
            output.extend_from_slice(input);
            return;
        }

        let in_len = input.len();
        while self.phase < in_len as f64 {
            let idx = self.phase.floor() as usize;
            let frac = (self.phase - idx as f64) as f32;

            let prev = if idx == 0 {
                self.last_frame
            } else {
                input[idx - 1]
            };
            let curr = input[idx];

            let l = prev[0] + (curr[0] - prev[0]) * frac;
            let r = prev[1] + (curr[1] - prev[1]) * frac;
            output.push([l, r]);

            self.phase += self.ratio;
        }

        if let Some(&last) = input.last() {
            self.last_frame = last;
        }
        self.phase -= in_len as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biquad_butterworth_lowpass_response() {
        let lpf = Biquad::lowpass(80.0, std::f64::consts::FRAC_1_SQRT_2, 44100.0);

        let gain_20 = lpf.gain_db_at(20.0);
        assert!(gain_20.abs() < 0.1, "Passband at 20 Hz should be ~0 dB, got {gain_20}");

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
        assert!(gain_20.abs() < 0.2, "20 Hz sub-bass should be ~0 dB, got {gain_20}");

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
        let mut dsp = SubwooferDsp::new(80.0, 0.0, 1.0, false, false, false, false, false, 44100);
        assert_eq!(dsp.delay_samples, 44);

        let mut block = vec![0.0f32; 88];
        dsp.process(&mut block);
        assert!(block.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn subwoofer_dsp_phase_inversion() {
        let mut dsp_norm = SubwooferDsp::new(80.0, 0.0, 0.0, true, false, false, false, false, 44100);
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
            assert!((a - b).abs() < 1e-6, "Bypass must leave samples un-filtered");
        }
    }

    #[test]
    fn soft_saturation_bounds() {
        assert_eq!(soft_saturate(0.5), 0.5);
        let sat = soft_saturate(2.0);
        assert!(sat < 1.0 && sat > 0.9);
        let sat_neg = soft_saturate(-2.0);
        assert!(sat_neg > -1.0 && sat_neg < -0.9);
    }

    #[test]
    fn resampler_identity() {
        let mut resampler = Resampler::new(48000, 48000);
        let input = vec![[0.1, 0.2], [0.3, 0.4]];
        let mut output = Vec::new();
        resampler.resample(&input, &mut output);
        assert_eq!(input, output);
    }
}
