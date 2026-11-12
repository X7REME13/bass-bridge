use crate::audio::AudioEngine;
use crate::settings::Settings;
use eframe::egui::{self, Color32, Pos2, Stroke, StrokeKind, Vec2};

pub struct BassBridgeApp {
    settings: Settings,
    audio: AudioEngine,
    output_devices: Vec<String>,
    input_devices: Vec<String>,
    last_error: Option<String>,
}

impl BassBridgeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        let output_devices = AudioEngine::list_output_devices();
        let input_devices = AudioEngine::list_input_devices();
        let audio = AudioEngine::new();

        let mut app = Self {
            settings,
            audio,
            output_devices,
            input_devices,
            last_error: None,
        };

        if app.settings.enabled {
            if let Err(e) = app.audio.start(&app.settings) {
                app.last_error = Some(e);
                app.settings.enabled = false;
            }
        }

        app
    }

    fn refresh_devices(&mut self) {
        self.output_devices = AudioEngine::list_output_devices();
        self.input_devices = AudioEngine::list_input_devices();
    }

    fn sync_dsp_parameters(&mut self) {
        if let Ok(mut dsp) = self.audio.dsp.lock() {
            dsp.set_cutoff(self.settings.cutoff_hz);
            dsp.set_gain_db(self.settings.gain_db);
            dsp.set_delay_ms(self.settings.delay_ms);
            dsp.set_stereo(self.settings.stereo);
            dsp.set_phase_inverted(self.settings.phase_inverted);
            dsp.set_bypass(self.settings.bypass);
        }
    }
}

impl eframe::App for BassBridgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request continuous repaint for smooth 60fps waveform animation when running
        if self.audio.is_running() {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);

            // 1. Header
            ui.horizontal(|ui| {
                ui.heading("🔊 BassBridge");
                ui.label(
                    egui::RichText::new("System Audio Subwoofer & Transducer Router")
                        .color(Color32::from_rgb(140, 140, 140)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.audio.is_running() {
                        ui.label(
                            egui::RichText::new("● ACTIVE")
                                .color(Color32::from_rgb(80, 220, 100))
                                .strong(),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("○ IDLE")
                                .color(Color32::from_rgb(160, 160, 160)),
                        );
                    }
                });
            });

            ui.separator();

            // 2. Audio Devices & Main Toggle
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Input (Loopback Capture):");
                    let current_input = self.settings.input_device.as_deref().unwrap_or("System Default Playback");
                    egui::ComboBox::from_id_salt("input-device-combo")
                        .selected_text(current_input)
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(self.settings.input_device.is_none(), "System Default Playback").clicked() {
                                self.settings.input_device = None;
                                self.settings.save();
                                if self.audio.is_running() {
                                    let _ = self.audio.start(&self.settings);
                                }
                            }

                            // List output devices (WASAPI loopback targets)
                            for dev in &self.output_devices {
                                let is_selected = self.settings.input_device.as_deref() == Some(dev);
                                if ui.selectable_label(is_selected, dev).clicked() {
                                    self.settings.input_device = Some(dev.clone());
                                    self.settings.save();
                                    if self.audio.is_running() {
                                        let _ = self.audio.start(&self.settings);
                                    }
                                }
                            }

                            // Also list input / monitor devices for Linux / PulseAudio
                            for dev in &self.input_devices {
                                if !self.output_devices.contains(dev) {
                                    let is_selected = self.settings.input_device.as_deref() == Some(dev);
                                    if ui.selectable_label(is_selected, format!("(Mic/In) {dev}")).clicked() {
                                        self.settings.input_device = Some(dev.clone());
                                        self.settings.save();
                                        if self.audio.is_running() {
                                            let _ = self.audio.start(&self.settings);
                                        }
                                    }
                                }
                            }
                        });

                    if ui.button("🔄").on_hover_text("Refresh audio devices").clicked() {
                        self.refresh_devices();
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Subwoofer Output Device:");
                    let current_output = self.settings.output_device.as_deref().unwrap_or("System Default Output");
                    egui::ComboBox::from_id_salt("output-device-combo")
                        .selected_text(current_output)
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(self.settings.output_device.is_none(), "System Default Output").clicked() {
                                self.settings.output_device = None;
                                self.settings.save();
                                if self.audio.is_running() {
                                    let _ = self.audio.start(&self.settings);
                                }
                            }
                            for dev in &self.output_devices {
                                let is_selected = self.settings.output_device.as_deref() == Some(dev);
                                if ui.selectable_label(is_selected, dev).clicked() {
                                    self.settings.output_device = Some(dev.clone());
                                    self.settings.save();
                                    if self.audio.is_running() {
                                        let _ = self.audio.start(&self.settings);
                                    }
                                }
                            }
                        });
                });

                ui.add_space(4.0);

                let is_on = self.audio.is_running();
                let toggle_text = if is_on { "■ Stop Bass Routing" } else { "▶ Start Bass Routing" };
                let toggle_color = if is_on { Color32::from_rgb(220, 60, 60) } else { Color32::from_rgb(60, 160, 240) };

                if ui.add(egui::Button::new(egui::RichText::new(toggle_text).color(Color32::WHITE).strong()).fill(toggle_color)).clicked() {
                    if is_on {
                        self.audio.stop();
                        self.settings.enabled = false;
                        self.settings.save();
                    } else {
                        match self.audio.start(&self.settings) {
                            Ok(_) => {
                                self.last_error = None;
                                self.settings.enabled = true;
                                self.settings.save();
                            }
                            Err(e) => {
                                self.last_error = Some(e);
                                self.settings.enabled = false;
                            }
                        }
                    }
                }
            });

            if let Some(ref err) = self.last_error {
                ui.colored_label(Color32::from_rgb(240, 80, 80), format!("⚠️ {err}"));
            }

            // 3. Live Waveform Visualizer
            ui.label(egui::RichText::new("Live Waveform Monitor:").strong());
            render_waveform_visualizer(ui, &self.audio);

            // 4. Subwoofer DSP Controls
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.heading("🎛️ DSP & Crossover Controls");

                // Stereo vs Mono Switch
                let mut stereo = self.settings.stereo;
                if ui.checkbox(&mut stereo, "Stereo Subwoofer (Dual-Channel for Chair Transducers)").changed() {
                    self.settings.stereo = stereo;
                    self.sync_dsp_parameters();
                    self.settings.save();
                }
                ui.label(
                    egui::RichText::new(
                        if stereo {
                            "Mode: Independent Left & Right channels driving chair shakers with spatial rumble."
                        } else {
                            "Mode: Summed (L+R)/2 Dual-Mono output feeding traditional subwoofer cabinet."
                        }
                    ).small().color(Color32::from_rgb(160, 160, 160)),
                );

                ui.separator();

                // Crossover Cutoff Slider
                ui.horizontal(|ui| {
                    ui.label("Crossover Cutoff (LPF):");
                    let mut cutoff = self.settings.cutoff_hz;
                    if ui.add(egui::Slider::new(&mut cutoff, 20.0..=1000.0).step_by(5.0).suffix(" Hz")).changed() {
                        self.settings.cutoff_hz = cutoff;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });

                // Gain Trim Slider
                ui.horizontal(|ui| {
                    ui.label("Subwoofer Gain Trim:");
                    let mut gain = self.settings.gain_db;
                    if ui.add(egui::Slider::new(&mut gain, -30.0..=30.0).step_by(0.5).suffix(" dB")).changed() {
                        self.settings.gain_db = gain;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });

                // Delay Compensation Slider
                ui.horizontal(|ui| {
                    ui.label("Delay Compensation:");
                    let mut delay = self.settings.delay_ms;
                    if ui.add(egui::Slider::new(&mut delay, 0.0..=500.0).step_by(1.0).suffix(" ms")).changed() {
                        self.settings.delay_ms = delay;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });

                ui.separator();

                // Phase Inversion & Bypass
                ui.horizontal(|ui| {
                    let mut phase_inv = self.settings.phase_inverted;
                    let phase_label = if phase_inv { "Phase: 180° (Inverted)" } else { "Phase: 0° (Normal)" };
                    if ui.selectable_label(phase_inv, phase_label).on_hover_text("Inverts audio polarity by 180° to eliminate acoustic cancellation between subwoofer and main speakers").clicked() {
                        self.settings.phase_inverted = !phase_inv;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }

                    let mut bypass = self.settings.bypass;
                    let bypass_label = if bypass { "DSP Bypassed (Full Range)" } else { "DSP Active (LR4 Filter)" };
                    if ui.selectable_label(bypass, bypass_label).on_hover_text("Bypasses low-pass filtering for A/B testing and direct comparison").clicked() {
                        self.settings.bypass = !bypass;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });
            });
        });
    }
}

fn render_waveform_visualizer(ui: &mut egui::Ui, audio: &AudioEngine) {
    let desired_size = Vec2::new(ui.available_width(), 110.0);
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Dark oscilloscope background
    painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 20, 24));
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, Color32::from_rgb(45, 50, 60)), StrokeKind::Inside);

    // Grid center line
    let mid_y = rect.center().y;
    painter.line_segment(
        [Pos2::new(rect.left(), mid_y), Pos2::new(rect.right(), mid_y)],
        Stroke::new(0.5, Color32::from_rgb(35, 40, 50)),
    );

    let in_samples = audio.tap.input_wave.lock().unwrap().clone();
    let bass_samples = audio.tap.bass_wave.lock().unwrap().clone();

    let n = in_samples.len().min(bass_samples.len()).max(2);
    let step_x = rect.width() / (n - 1) as f32;
    let amp = rect.height() * 0.42;

    // Draw Input Wave (Cyan / Full Range)
    let in_points: Vec<Pos2> = in_samples
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, &s)| {
            let x = rect.left() + i as f32 * step_x;
            let y = mid_y - s.clamp(-1.0, 1.0) * amp;
            Pos2::new(x, y)
        })
        .collect();

    if in_points.len() >= 2 {
        painter.add(egui::Shape::line(
            in_points,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 200, 255, 120)),
        ));
    }

    // Draw Subwoofer Bass Wave (Neon Orange / Filtered Low Frequencies)
    let bass_points: Vec<Pos2> = bass_samples
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, &s)| {
            let x = rect.left() + i as f32 * step_x;
            let y = mid_y - s.clamp(-1.0, 1.0) * amp;
            Pos2::new(x, y)
        })
        .collect();

    if bass_points.len() >= 2 {
        painter.add(egui::Shape::line(
            bass_points,
            Stroke::new(2.2, Color32::from_rgb(255, 140, 30)),
        ));
    }

    // Legend
    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "■ Input Full-Range Audio",
        egui::FontId::proportional(11.0),
        Color32::from_rgb(80, 200, 255),
    );
    painter.text(
        Pos2::new(rect.left() + 160.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "■ Filtered Bass (Subwoofer / Chair Shakers)",
        egui::FontId::proportional(11.0),
        Color32::from_rgb(255, 140, 30),
    );
}
