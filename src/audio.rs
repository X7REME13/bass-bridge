use crate::dsp::{AdaptiveResampler, SubwooferDsp, FFT_SIZE};
use crate::settings::Settings;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, Stream, StreamConfig};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

trait PcmSample: SizedSample {
    fn to_unit_f32(self) -> f32;
    fn from_unit_f32(v: f32) -> Self;
}

impl PcmSample for f32 {
    #[inline]
    fn to_unit_f32(self) -> f32 {
        if self.is_finite() {
            self
        } else {
            0.0
        }
    }

    #[inline]
    fn from_unit_f32(v: f32) -> Self {
        if v.is_finite() {
            v
        } else {
            0.0
        }
    }
}

impl PcmSample for i16 {
    #[inline]
    fn to_unit_f32(self) -> f32 {
        self as f32 / 32768.0
    }

    #[inline]
    fn from_unit_f32(v: f32) -> Self {
        let clamped = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        (clamped * 32767.0).round() as i16
    }
}

impl PcmSample for i32 {
    #[inline]
    fn to_unit_f32(self) -> f32 {
        (self as f64 / 2147483648.0) as f32
    }

    #[inline]
    fn from_unit_f32(v: f32) -> Self {
        let clamped = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        (clamped as f64 * 2147483647.0).round() as i32
    }
}

impl PcmSample for u16 {
    #[inline]
    fn to_unit_f32(self) -> f32 {
        (self as f32 - 32768.0) / 32768.0
    }

    #[inline]
    fn from_unit_f32(v: f32) -> Self {
        let clamped = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        ((clamped * 32767.0) + 32768.0).round() as u16
    }
}

/// Lock-free atomic bridge transferring DSP parameters from the UI thread to the audio thread.
pub struct SharedDspParams {
    cutoff_hz: AtomicU32,
    gain_db: AtomicU32,
    delay_ms: AtomicU32,
    stereo: AtomicBool,
    phase_inverted: AtomicBool,
    bypass: AtomicBool,
    limiter: AtomicBool,
    direct_sum: AtomicBool,
}

impl SharedDspParams {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            cutoff_hz: AtomicU32::new(settings.cutoff_hz.to_bits()),
            gain_db: AtomicU32::new(settings.gain_db.to_bits()),
            delay_ms: AtomicU32::new(settings.delay_ms.to_bits()),
            stereo: AtomicBool::new(settings.stereo),
            phase_inverted: AtomicBool::new(settings.phase_inverted),
            bypass: AtomicBool::new(settings.bypass),
            limiter: AtomicBool::new(settings.limiter),
            direct_sum: AtomicBool::new(settings.direct_sum),
        }
    }

    pub fn update_from_settings(&self, settings: &Settings) {
        self.cutoff_hz
            .store(settings.cutoff_hz.to_bits(), Ordering::Relaxed);
        self.gain_db
            .store(settings.gain_db.to_bits(), Ordering::Relaxed);
        self.delay_ms
            .store(settings.delay_ms.to_bits(), Ordering::Relaxed);
        self.stereo.store(settings.stereo, Ordering::Relaxed);
        self.phase_inverted
            .store(settings.phase_inverted, Ordering::Relaxed);
        self.bypass.store(settings.bypass, Ordering::Relaxed);
        self.limiter.store(settings.limiter, Ordering::Relaxed);
        self.direct_sum
            .store(settings.direct_sum, Ordering::Relaxed);
    }

    pub fn apply_to(&self, dsp: &mut SubwooferDsp) {
        dsp.set_cutoff(f32::from_bits(self.cutoff_hz.load(Ordering::Relaxed)));
        dsp.set_gain_db(f32::from_bits(self.gain_db.load(Ordering::Relaxed)));
        dsp.set_delay_ms(f32::from_bits(self.delay_ms.load(Ordering::Relaxed)));
        dsp.set_stereo(self.stereo.load(Ordering::Relaxed));
        dsp.set_phase_inverted(self.phase_inverted.load(Ordering::Relaxed));
        dsp.set_bypass(self.bypass.load(Ordering::Relaxed));
        dsp.set_limiter(self.limiter.load(Ordering::Relaxed));
        dsp.set_direct_sum(self.direct_sum.load(Ordering::Relaxed));
    }
}

struct RingWave {
    buf: Vec<f32>,
    write_pos: usize,
}

impl RingWave {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len],
            write_pos: 0,
        }
    }

    fn push(&mut self, samples: &[f32]) {
        let cap = self.buf.len();
        if samples.is_empty() || cap == 0 {
            return;
        }
        if samples.len() >= cap {
            self.buf.copy_from_slice(&samples[samples.len() - cap..]);
            self.write_pos = 0;
            return;
        }
        let first = (cap - self.write_pos).min(samples.len());
        self.buf[self.write_pos..self.write_pos + first].copy_from_slice(&samples[..first]);
        let rem = samples.len() - first;
        if rem > 0 {
            self.buf[..rem].copy_from_slice(&samples[first..]);
            self.write_pos = rem;
        } else {
            self.write_pos = (self.write_pos + first) % cap;
        }
    }

    fn snapshot_into(&self, dst: &mut [f32]) {
        let cap = self.buf.len();
        if cap == 0 || dst.is_empty() {
            return;
        }
        let n = dst.len().min(cap);
        let start = (self.write_pos + cap - n) % cap;
        let first = (cap - start).min(n);
        dst[..first].copy_from_slice(&self.buf[start..start + first]);
        if first < n {
            dst[first..].copy_from_slice(&self.buf[..n - first]);
        }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.write_pos = 0;
    }
}

#[inline]
fn atomic_max_f32(target: &AtomicU32, val: f32) {
    if !val.is_finite() {
        return;
    }
    let mut cur = target.load(Ordering::Relaxed);
    loop {
        let cur_f = f32::from_bits(cur);
        if val <= cur_f {
            break;
        }
        match target.compare_exchange_weak(
            cur,
            val.to_bits(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next) => cur = next,
        }
    }
}

/// Real-time 4096-sample ring buffers and peak-hold atomics for FFT spectrum & oscilloscope UI.
#[derive(Clone)]
pub struct VisualizerTap {
    input_ring: Arc<Mutex<RingWave>>,
    bass_ring: Arc<Mutex<RingWave>>,
    peak_in_bits: Arc<AtomicU32>,
    peak_out_bits: Arc<AtomicU32>,
}

impl Default for VisualizerTap {
    fn default() -> Self {
        Self::new()
    }
}

impl VisualizerTap {
    pub fn new() -> Self {
        Self {
            input_ring: Arc::new(Mutex::new(RingWave::new(FFT_SIZE))),
            bass_ring: Arc::new(Mutex::new(RingWave::new(FFT_SIZE))),
            peak_in_bits: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            peak_out_bits: Arc::new(AtomicU32::new(0.0f32.to_bits())),
        }
    }

    pub fn push_input(&self, samples: &[f32]) {
        let mut max_abs = 0.0f32;
        for &s in samples {
            let a = s.abs();
            if a > max_abs {
                max_abs = a;
            }
        }
        atomic_max_f32(&self.peak_in_bits, max_abs);
        if let Ok(mut ring) = self.input_ring.try_lock() {
            ring.push(samples);
        }
    }

    pub fn push_bass(&self, samples: &[f32]) {
        let mut max_abs = 0.0f32;
        for &s in samples {
            let a = s.abs();
            if a > max_abs {
                max_abs = a;
            }
        }
        atomic_max_f32(&self.peak_out_bits, max_abs);
        if let Ok(mut ring) = self.bass_ring.try_lock() {
            ring.push(samples);
        }
    }

    pub fn take_peaks(&self) -> (f32, f32) {
        let p_in = f32::from_bits(self.peak_in_bits.swap(0.0f32.to_bits(), Ordering::Relaxed));
        let p_out = f32::from_bits(self.peak_out_bits.swap(0.0f32.to_bits(), Ordering::Relaxed));
        (p_in, p_out)
    }

    pub fn snapshot_input(&self, dst: &mut [f32]) {
        if let Ok(ring) = self.input_ring.lock() {
            ring.snapshot_into(dst);
        }
    }

    pub fn snapshot_bass(&self, dst: &mut [f32]) {
        if let Ok(ring) = self.bass_ring.lock() {
            ring.snapshot_into(dst);
        }
    }

    pub fn clear(&self) {
        self.peak_in_bits
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.peak_out_bits
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        if let Ok(mut ring) = self.input_ring.lock() {
            ring.clear();
        }
        if let Ok(mut ring) = self.bass_ring.lock() {
            ring.clear();
        }
    }
}

pub struct AudioEngine {
    _capture_stream: Option<Stream>,
    _playback_stream: Option<Stream>,
    pub running: Arc<AtomicBool>,
    pub params: Arc<SharedDspParams>,
    pub tap: VisualizerTap,
    pub in_rate: Arc<AtomicU32>,
    pub out_rate: Arc<AtomicU32>,
    pub input_error: Arc<Mutex<Option<String>>>,
    pub output_error: Arc<Mutex<Option<String>>>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            _capture_stream: None,
            _playback_stream: None,
            running: Arc::new(AtomicBool::new(false)),
            params: Arc::new(SharedDspParams::from_settings(&Settings::default())),
            tap: VisualizerTap::new(),
            in_rate: Arc::new(AtomicU32::new(0)),
            out_rate: Arc::new(AtomicU32::new(0)),
            input_error: Arc::new(Mutex::new(None)),
            output_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn list_output_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.output_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    pub fn list_input_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.input_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    pub fn default_output_device_name() -> Option<String> {
        cpal::default_host()
            .default_output_device()
            .and_then(|d| d.name().ok())
    }

    pub fn sync_settings(&self, settings: &Settings) {
        self.params.update_from_settings(settings);
    }

    pub fn active_rates(&self) -> (u32, u32) {
        (
            self.in_rate.load(Ordering::Relaxed),
            self.out_rate.load(Ordering::Relaxed),
        )
    }

    pub fn stream_error(&self) -> Option<String> {
        let in_err = self.input_error.lock().ok().and_then(|e| e.clone());
        let out_err = self.output_error.lock().ok().and_then(|e| e.clone());
        match (in_err, out_err) {
            (Some(a), Some(b)) => Some(format!("{a} | {b}")),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    pub fn start(&mut self, settings: &Settings) -> Result<(), String> {
        self.stop();
        self.params.update_from_settings(settings);

        let host = cpal::default_host();

        // 1. Resolve Capture Device (WASAPI loopback on an output device, or physical input device)
        let (input_device, input_is_loopback) = if let Some(ref name) = settings.input_device {
            if let Some(dev) = host
                .output_devices()
                .ok()
                .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(name)))
            {
                (dev, true)
            } else if let Some(dev) = host
                .input_devices()
                .ok()
                .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(name)))
            {
                (dev, false)
            } else if let Some(dev) = host.default_output_device() {
                (dev, true)
            } else {
                return Err(format!("Capture device \"{name}\" not found."));
            }
        } else if let Some(dev) = host.default_output_device() {
            (dev, true)
        } else if let Some(dev) = host.default_input_device() {
            (dev, false)
        } else {
            return Err("No default playback device found for loopback capture.".to_string());
        };

        // 2. Resolve Subwoofer Output Device
        let output_device = if let Some(ref name) = settings.output_device {
            host.output_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.name().ok().as_deref() == Some(name))
                .or_else(|| host.default_output_device())
        } else {
            host.default_output_device()
        }
        .ok_or_else(|| "No output device found for subwoofer routing.".to_string())?;

        // 3. Feedback Loop Guard: prevent capturing WASAPI loopback and playing back into the same endpoint
        let in_name = input_device.name().ok();
        let out_name = output_device.name().ok();
        if input_is_loopback && in_name.is_some() && in_name == out_name {
            return Err(format!(
                "Input and output are both set to \"{}\". Select a different secondary output device to avoid an audio feedback loop.",
                in_name.unwrap()
            ));
        }

        let in_config = if input_is_loopback {
            input_device
                .default_output_config()
                .or_else(|_| input_device.default_input_config())
        } else {
            input_device
                .default_input_config()
                .or_else(|_| input_device.default_output_config())
        }
        .map_err(|e| format!("Failed to read capture format: {e}"))?;

        let out_config = output_device
            .default_output_config()
            .map_err(|e| format!("Failed to read output format: {e}"))?;

        let in_rate = in_config.sample_rate().0;
        let out_rate = out_config.sample_rate().0;
        let in_channels = (in_config.channels() as usize).max(1);
        let out_channels = (out_config.channels() as usize).max(1);
        let in_format = in_config.sample_format();
        let out_format = out_config.sample_format();
        let stream_config_in = in_config.config();
        let stream_config_out = out_config.config();

        let (tx, rx): (Sender<[f32; 2]>, Receiver<[f32; 2]>) = bounded(16384);

        let tap_in = self.tap.clone();
        let in_error = self.input_error.clone();
        if let Ok(mut e) = in_error.lock() {
            *e = None;
        }

        let capture_stream = match in_format {
            SampleFormat::F32 => build_capture_stream::<f32>(
                &input_device,
                &stream_config_in,
                in_channels,
                tx,
                tap_in,
                in_error,
            ),
            SampleFormat::I16 => build_capture_stream::<i16>(
                &input_device,
                &stream_config_in,
                in_channels,
                tx,
                tap_in,
                in_error,
            ),
            SampleFormat::I32 => build_capture_stream::<i32>(
                &input_device,
                &stream_config_in,
                in_channels,
                tx,
                tap_in,
                in_error,
            ),
            SampleFormat::U16 => build_capture_stream::<u16>(
                &input_device,
                &stream_config_in,
                in_channels,
                tx,
                tap_in,
                in_error,
            ),
            other => {
                return Err(format!("Unsupported capture audio format: {other:?}"));
            }
        }
        .map_err(|e| format!("Failed to create capture stream: {e}"))?;

        let params_shared = self.params.clone();
        let tap_bass = self.tap.clone();
        let out_error = self.output_error.clone();
        if let Ok(mut e) = out_error.lock() {
            *e = None;
        }

        let mut dsp = SubwooferDsp::new(
            settings.cutoff_hz,
            settings.gain_db,
            settings.delay_ms,
            settings.stereo,
            settings.phase_inverted,
            settings.bypass,
            settings.limiter,
            settings.direct_sum,
            out_rate,
        );
        params_shared.apply_to(&mut dsp);

        let playback_stream = match out_format {
            SampleFormat::F32 => build_playback_stream::<f32>(
                &output_device,
                &stream_config_out,
                in_rate,
                out_rate,
                out_channels,
                rx,
                dsp,
                params_shared,
                tap_bass,
                out_error,
            ),
            SampleFormat::I16 => build_playback_stream::<i16>(
                &output_device,
                &stream_config_out,
                in_rate,
                out_rate,
                out_channels,
                rx,
                dsp,
                params_shared,
                tap_bass,
                out_error,
            ),
            SampleFormat::I32 => build_playback_stream::<i32>(
                &output_device,
                &stream_config_out,
                in_rate,
                out_rate,
                out_channels,
                rx,
                dsp,
                params_shared,
                tap_bass,
                out_error,
            ),
            SampleFormat::U16 => build_playback_stream::<u16>(
                &output_device,
                &stream_config_out,
                in_rate,
                out_rate,
                out_channels,
                rx,
                dsp,
                params_shared,
                tap_bass,
                out_error,
            ),
            other => {
                return Err(format!("Unsupported output audio format: {other:?}"));
            }
        }
        .map_err(|e| format!("Failed to create subwoofer output stream: {e}"))?;

        capture_stream.play().map_err(|e| e.to_string())?;
        playback_stream.play().map_err(|e| e.to_string())?;

        self.in_rate.store(in_rate, Ordering::Relaxed);
        self.out_rate.store(out_rate, Ordering::Relaxed);
        self._capture_stream = Some(capture_stream);
        self._playback_stream = Some(playback_stream);
        self.running.store(true, Ordering::SeqCst);

        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self._capture_stream = None;
        self._playback_stream = None;
        self.in_rate.store(0, Ordering::Relaxed);
        self.out_rate.store(0, Ordering::Relaxed);
        self.tap.clear();
        if let Ok(mut e) = self.input_error.lock() {
            *e = None;
        }
        if let Ok(mut e) = self.output_error.lock() {
            *e = None;
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

fn build_capture_stream<S: PcmSample + 'static>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    tx: Sender<[f32; 2]>,
    tap_in: VisualizerTap,
    error_slot: Arc<Mutex<Option<String>>>,
) -> Result<Stream, cpal::BuildStreamError> {
    let mut viz_buf: Vec<f32> = Vec::with_capacity(2048);
    device.build_input_stream(
        config,
        move |data: &[S], _: &cpal::InputCallbackInfo| {
            viz_buf.clear();
            for frame in data.chunks_exact(channels) {
                let l = frame[0].to_unit_f32();
                let r = if channels > 1 {
                    frame[1].to_unit_f32()
                } else {
                    l
                };
                let _ = tx.try_send([l, r]);
                viz_buf.push((l + r) * 0.5);
            }
            tap_in.push_input(&viz_buf);
        },
        move |err| {
            if let Ok(mut slot) = error_slot.lock() {
                *slot = Some(format!("Capture error: {err}"));
            }
        },
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_playback_stream<S: PcmSample + 'static>(
    device: &cpal::Device,
    config: &StreamConfig,
    in_rate: u32,
    out_rate: u32,
    out_channels: usize,
    rx: Receiver<[f32; 2]>,
    mut dsp: SubwooferDsp,
    params: Arc<SharedDspParams>,
    tap_bass: VisualizerTap,
    error_slot: Arc<Mutex<Option<String>>>,
) -> Result<Stream, cpal::BuildStreamError> {
    let mut resampler = AdaptiveResampler::new(in_rate, out_rate);
    let base_ratio = resampler.base_ratio();
    let mut fifo: VecDeque<[f32; 2]> = VecDeque::with_capacity(16384);
    let mut viz_buf: Vec<f32> = Vec::with_capacity(2048);
    let mut primed = false;
    let mut ramp = 0.0f32;
    let mut err_smooth = 0.0f64;

    device.build_output_stream(
        config,
        move |data: &mut [S], _: &cpal::OutputCallbackInfo| {
            params.apply_to(&mut dsp);

            while let Ok(frame) = rx.try_recv() {
                fifo.push_back(frame);
            }

            let needed_frames = (data.len() / out_channels).max(1);
            let min_target = ((in_rate as usize * 20) / 1000).max(256);
            let target_frames = ((needed_frames as f64 * base_ratio).ceil() as usize * 2)
                .max(min_target);

            // Overrun guard: prevent unbound latency buildup if clocks drift apart
            let max_fifo = target_frames * 4;
            if fifo.len() > max_fifo {
                let drop_count = fifo.len() - target_frames;
                fifo.drain(..drop_count);
            }

            if !primed && fifo.len() >= target_frames {
                primed = true;
                ramp = 0.0;
                err_smooth = 0.0;
            }

            let step_ratio = if primed {
                let fill_err =
                    (fifo.len() as f64 - target_frames as f64) / (target_frames as f64).max(1.0);
                err_smooth += 0.02 * (fill_err - err_smooth);
                base_ratio * (1.0 + (err_smooth * 0.0025).clamp(-0.004, 0.004))
            } else {
                base_ratio
            };

            viz_buf.clear();
            let zero = S::from_unit_f32(0.0);

            for frame in data.chunks_exact_mut(out_channels) {
                let raw = if primed {
                    match resampler.next_frame(&mut fifo, step_ratio) {
                        Some(f) => {
                            ramp = (ramp + (1.0 / 64.0)).min(1.0);
                            [f[0] * ramp, f[1] * ramp]
                        }
                        None => {
                            primed = false;
                            ramp = 0.0;
                            resampler.reset();
                            [0.0, 0.0]
                        }
                    }
                } else {
                    [0.0, 0.0]
                };

                let out = dsp.process_frame(raw);
                frame.fill(zero);
                frame[0] = S::from_unit_f32(out[0]);
                if out_channels > 1 {
                    frame[1] = S::from_unit_f32(out[1]);
                }
                viz_buf.push((out[0] + out[1]) * 0.5);
            }

            tap_bass.push_bass(&viz_buf);
        },
        move |err| {
            if let Ok(mut slot) = error_slot.lock() {
                *slot = Some(format!("Playback error: {err}"));
            }
        },
        None,
    )
}
