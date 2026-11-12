use crate::dsp::{Resampler, SubwooferDsp};
use crate::settings::Settings;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Snapshot buffer for real-time oscilloscope display.
#[derive(Clone, Default)]
pub struct VisualizerTap {
    pub input_wave: Arc<Mutex<Vec<f32>>>,
    pub bass_wave: Arc<Mutex<Vec<f32>>>,
}

impl VisualizerTap {
    pub fn new() -> Self {
        Self {
            input_wave: Arc::new(Mutex::new(vec![0.0; 256])),
            bass_wave: Arc::new(Mutex::new(vec![0.0; 256])),
        }
    }

    pub fn push_input(&self, samples: &[f32]) {
        if let Ok(mut buf) = self.input_wave.try_lock() {
            let n = samples.len().min(buf.len());
            buf.drain(0..n);
            buf.extend_from_slice(&samples[samples.len() - n..]);
        }
    }

    pub fn push_bass(&self, samples: &[f32]) {
        if let Ok(mut buf) = self.bass_wave.try_lock() {
            let n = samples.len().min(buf.len());
            buf.drain(0..n);
            buf.extend_from_slice(&samples[samples.len() - n..]);
        }
    }
}

pub struct AudioEngine {
    _capture_stream: Option<Stream>,
    _playback_stream: Option<Stream>,
    pub running: Arc<AtomicBool>,
    pub dsp: Arc<Mutex<SubwooferDsp>>,
    pub tap: VisualizerTap,
    pub input_error: Arc<Mutex<Option<String>>>,
    pub output_error: Arc<Mutex<Option<String>>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            _capture_stream: None,
            _playback_stream: None,
            running: Arc::new(AtomicBool::new(false)),
            dsp: Arc::new(Mutex::new(SubwooferDsp::new(80.0, 0.0, 0.0, false, false, false, 48000))),
            tap: VisualizerTap::new(),
            input_error: Arc::new(Mutex::new(None)),
            output_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn list_output_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.output_devices()
            .map(|devices| {
                devices
                    .filter_map(|d| d.name().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn list_input_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.input_devices()
            .map(|devices| {
                devices
                    .filter_map(|d| d.name().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn start(&mut self, settings: &Settings) -> Result<(), String> {
        self.stop();

        let host = cpal::default_host();

        // 1. Locate Capture Device (WASAPI loopback on Windows output device, or monitor input device)
        let input_device = if let Some(ref name) = settings.input_device {
            host.output_devices()
                .ok()
                .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(name)))
                .or_else(|| {
                    host.input_devices()
                        .ok()
                        .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(name)))
                })
                .or_else(|| host.default_output_device())
        } else {
            host.default_output_device().or_else(|| host.default_input_device())
        }
        .ok_or_else(|| "No default playback device found for audio loopback capture".to_string())?;

        // 2. Locate Subwoofer Output Device
        let output_device = if let Some(ref name) = settings.output_device {
            host.output_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.name().ok().as_deref() == Some(name))
                .or_else(|| host.default_output_device())
        } else {
            host.default_output_device()
        }
        .ok_or_else(|| "No output device found for subwoofer routing".to_string())?;

        let in_config = input_device
            .default_output_config()
            .or_else(|_| input_device.default_input_config())
            .map_err(|e| e.to_string())?;
        let out_config = output_device.default_output_config().map_err(|e| e.to_string())?;

        let in_rate = in_config.sample_rate().0;
        let out_rate = out_config.sample_rate().0;
        let in_channels = in_config.channels() as usize;
        let out_channels = out_config.channels() as usize;
        let in_format = in_config.sample_format();
        let out_format = out_config.sample_format();
        let stream_config_in = in_config.config();
        let stream_config_out = out_config.config();

        // Initialize DSP matching the hardware output sample rate
        *self.dsp.lock().unwrap() = SubwooferDsp::new(
            settings.cutoff_hz,
            settings.gain_db,
            settings.delay_ms,
            settings.stereo,
            settings.phase_inverted,
            settings.bypass,
            out_rate,
        );

        // Low-latency inter-stream ring buffer (channel of stereo frames [f32; 2])
        let (tx, rx): (Sender<[f32; 2]>, Receiver<[f32; 2]>) = bounded(8192);

        let tap_in = self.tap.clone();
        let in_error = self.input_error.clone();
        *in_error.lock().unwrap() = None;

        // 3. Build Loopback Input Stream on the capture device
        let capture_stream = match in_format {
            SampleFormat::F32 => {
                input_device.build_input_stream(
                    &stream_config_in,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mut viz_samples = Vec::with_capacity(data.len() / in_channels);
                        for frame in data.chunks_exact(in_channels) {
                            let l = frame[0];
                            let r = if in_channels > 1 { frame[1] } else { frame[0] };
                            let _ = tx.try_send([l, r]);
                            viz_samples.push((l + r) * 0.5);
                        }
                        tap_in.push_input(&viz_samples);
                    },
                    move |err| {
                        *in_error.lock().unwrap() = Some(format!("Capture error: {err}"));
                    },
                    None,
                )
            }
            SampleFormat::I16 => {
                input_device.build_input_stream(
                    &stream_config_in,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let mut viz_samples = Vec::with_capacity(data.len() / in_channels);
                        for frame in data.chunks_exact(in_channels) {
                            let l = frame[0] as f32 / 32768.0;
                            let r = if in_channels > 1 { frame[1] as f32 / 32768.0 } else { l };
                            let _ = tx.try_send([l, r]);
                            viz_samples.push((l + r) * 0.5);
                        }
                        tap_in.push_input(&viz_samples);
                    },
                    move |err| {
                        *in_error.lock().unwrap() = Some(format!("Capture error: {err}"));
                    },
                    None,
                )
            }
            _ => return Err("Unsupported capture audio format (only F32 and I16 supported)".into()),
        }
        .map_err(|e| format!("Failed to create loopback capture stream: {e}"))?;

        // 4. Build Subwoofer Output Stream
        let dsp_shared = self.dsp.clone();
        let tap_bass = self.tap.clone();
        let out_error = self.output_error.clone();
        *out_error.lock().unwrap() = None;

        let mut resampler = Resampler::new(in_rate, out_rate);
        let mut resampled_buf = Vec::with_capacity(4096);

        let playback_stream = match out_format {
            SampleFormat::F32 => output_device.build_output_stream(
                &stream_config_out,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let needed_frames = data.len() / out_channels;
                    resampled_buf.clear();

                    // Pull captured input frames
                    let mut input_frames = Vec::with_capacity(needed_frames);
                    while let Ok(frame) = rx.try_recv() {
                        input_frames.push(frame);
                        if input_frames.len() >= needed_frames * 2 {
                            break;
                        }
                    }

                    // Resample to subwoofer device rate if needed
                    resampler.resample(&input_frames, &mut resampled_buf);

                    let mut dsp = dsp_shared.lock().unwrap();
                    let mut viz_bass = Vec::with_capacity(needed_frames);

                    for (i, frame) in data.chunks_exact_mut(out_channels).enumerate() {
                        if i < resampled_buf.len() {
                            let mut pair = [resampled_buf[i][0], resampled_buf[i][1]];
                            dsp.process(&mut pair);
                            frame[0] = pair[0];
                            if out_channels > 1 {
                                frame[1] = pair[1];
                            }
                            viz_bass.push((pair[0] + pair[1]) * 0.5);
                        } else {
                            frame.fill(0.0);
                        }
                    }
                    tap_bass.push_bass(&viz_bass);
                },
                move |err| {
                    *out_error.lock().unwrap() = Some(format!("Playback error: {err}"));
                },
                None,
            ),
            SampleFormat::I16 => output_device.build_output_stream(
                &stream_config_out,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let needed_frames = data.len() / out_channels;
                    resampled_buf.clear();

                    let mut input_frames = Vec::with_capacity(needed_frames);
                    while let Ok(frame) = rx.try_recv() {
                        input_frames.push(frame);
                        if input_frames.len() >= needed_frames * 2 {
                            break;
                        }
                    }

                    resampler.resample(&input_frames, &mut resampled_buf);

                    let mut dsp = dsp_shared.lock().unwrap();
                    let mut viz_bass = Vec::with_capacity(needed_frames);

                    for (i, frame) in data.chunks_exact_mut(out_channels).enumerate() {
                        if i < resampled_buf.len() {
                            let mut pair = [resampled_buf[i][0], resampled_buf[i][1]];
                            dsp.process(&mut pair);
                            frame[0] = (pair[0].clamp(-1.0, 1.0) * 32767.0) as i16;
                            if out_channels > 1 {
                                frame[1] = (pair[1].clamp(-1.0, 1.0) * 32767.0) as i16;
                            }
                            viz_bass.push((pair[0] + pair[1]) * 0.5);
                        } else {
                            frame.fill(0);
                        }
                    }
                    tap_bass.push_bass(&viz_bass);
                },
                move |err| {
                    *out_error.lock().unwrap() = Some(format!("Playback error: {err}"));
                },
                None,
            ),
            _ => return Err("Unsupported output format (only F32 and I16 supported)".into()),
        }
        .map_err(|e| format!("Failed to create subwoofer output stream: {e}"))?;

        capture_stream.play().map_err(|e| e.to_string())?;
        playback_stream.play().map_err(|e| e.to_string())?;

        self._capture_stream = Some(capture_stream);
        self._playback_stream = Some(playback_stream);
        self.running.store(true, Ordering::SeqCst);

        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self._capture_stream = None;
        self._playback_stream = None;
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}
