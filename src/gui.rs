use crate::audio::AudioEngine;
use crate::settings::Settings;
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};

#[derive(PartialEq, Clone, Copy)]
enum VisualizerTab {
    FrequencyResponse,
    Oscilloscope,
}

pub struct BassBridgeApp {
    settings: Settings,
    audio: AudioEngine,
    output_devices: Vec<String>,
    input_devices: Vec<String>,
    last_error: Option<String>,
    active_tab: VisualizerTab,
}

impl BassBridgeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set custom dark cyberpunk-studio visual theme
        Self::apply_studio_theme(&cc.egui_ctx);

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
            active_tab: VisualizerTab::FrequencyResponse,
        };

        if app.settings.enabled {
            if let Err(e) = app.audio.start(&app.settings) {
                app.last_error = Some(e);
                app.settings.enabled = false;
            }
        }

        app
    }

    fn apply_studio_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.dark_mode = true;
        visuals.panel_fill = Color32::from_rgb(11, 14, 18);
        visuals.window_fill = Color32::from_rgb(11, 14, 18);
        visuals.extreme_bg_color = Color32::from_rgb(7, 9, 12);
        visuals.faint_bg_color = Color32::from_rgb(16, 20, 27);
        visuals.override_text_color = Some(Color32::from_rgb(240, 244, 248));

        // Modern card frames
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(17, 21, 29);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(32, 40, 54));
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(6.0);

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(22, 28, 38);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(40, 50, 68));
        visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(30, 39, 54);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.2_f32, Color32::from_rgb(0, 229, 255));
        visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);

        visuals.widgets.active.bg_fill = Color32::from_rgb(38, 50, 70);
        visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, Color32::from_rgb(0, 229, 255));
        visuals.widgets.active.rounding = egui::Rounding::same(6.0);

        ctx.set_visuals(visuals);
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
            dsp.set_soft_clip(self.settings.soft_clip);
            dsp.set_direct_sum(self.settings.direct_sum);
        }
    }
}

impl eframe::App for BassBridgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Continuous repaint for 60 FPS animation when active
        if self.audio.is_running() {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);

            // ==========================================
            // 1. TOP STUDIO HEADER
            // ==========================================
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("🔊 BASSBRIDGE")
                        .size(20.0)
                        .strong()
                        .color(Color32::from_rgb(0, 229, 255)),
                );
                ui.label(
                    egui::RichText::new("PRO")
                        .size(10.0)
                        .strong()
                        .color(Color32::from_rgb(255, 122, 0)),
                );
                ui.label(
                    egui::RichText::new("• System Subwoofer & Tactile Transducer Crossover")
                        .size(12.0)
                        .color(Color32::from_rgb(140, 150, 165)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.audio.is_running() {
                        ui.label(
                            egui::RichText::new("● ACTIVE (60 FPS)")
                                .size(11.0)
                                .color(Color32::from_rgb(0, 230, 118))
                                .strong(),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("○ STANDBY")
                                .size(11.0)
                                .color(Color32::from_rgb(110, 120, 135)),
                        );
                    }
                });
            });

            // ==========================================
            // 2. HARDWARE I/O & MAIN ENGINE TOGGLE
            // ==========================================
            render_card(ui, "🔀 AUDIO HARDWARE ROUTING", |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Input Source:").strong());
                    let current_input = self.settings.input_device.as_deref().unwrap_or("System Default Playback");
                    egui::ComboBox::from_id_salt("input-device-combo")
                        .selected_text(current_input)
                        .width(280.0)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(self.settings.input_device.is_none(), "System Default Playback").clicked() {
                                self.settings.input_device = None;
                                self.settings.save();
                                if self.audio.is_running() {
                                    let _ = self.audio.start(&self.settings);
                                }
                            }
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
                            for dev in &self.input_devices {
                                if !self.output_devices.contains(dev) {
                                    let is_selected = self.settings.input_device.as_deref() == Some(dev);
                                    if ui.selectable_label(is_selected, format!("(In) {dev}")).clicked() {
                                        self.settings.input_device = Some(dev.clone());
                                        self.settings.save();
                                        if self.audio.is_running() {
                                            let _ = self.audio.start(&self.settings);
                                        }
                                    }
                                }
                            }
                        });

                    if ui.button("🔄").on_hover_text("Refresh connected audio devices").clicked() {
                        self.refresh_devices();
                    }
                });

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Sub / Shaker Out:").strong());
                    let current_output = self.settings.output_device.as_deref().unwrap_or("System Default Output");
                    egui::ComboBox::from_id_salt("output-device-combo")
                        .selected_text(current_output)
                        .width(280.0)
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
                let toggle_text = if is_on { "■ STOP BASS ROUTING" } else { "▶ START BASS ROUTING" };
                let toggle_color = if is_on {
                    Color32::from_rgb(225, 45, 65)
                } else {
                    Color32::from_rgb(0, 180, 216)
                };

                let btn = egui::Button::new(
                    egui::RichText::new(toggle_text)
                        .color(Color32::WHITE)
                        .size(13.0)
                        .strong(),
                )
                .fill(toggle_color)
                .min_size(Vec2::new(ui.available_width(), 32.0));

                if ui.add(btn).clicked() {
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
                ui.colored_label(Color32::from_rgb(255, 75, 95), format!("⚠️ {err}"));
            }

            // ==========================================
            // 3. STUDIO VISUALIZER (BODE PLOT & OSCILLOSCOPE)
            // ==========================================
            render_card(ui, "📊 REAL-TIME SPECTRUM & WAVE MONITOR", |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.active_tab == VisualizerTab::FrequencyResponse, "📈 Frequency Response (Bode)").clicked() {
                        self.active_tab = VisualizerTab::FrequencyResponse;
                    }
                    if ui.selectable_label(self.active_tab == VisualizerTab::Oscilloscope, "🌊 Dual Oscilloscope").clicked() {
                        self.active_tab = VisualizerTab::Oscilloscope;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (peak_in, peak_out) = self.audio.tap.get_peaks();
                        render_mini_peak_badge(ui, "IN", peak_in, Color32::from_rgb(0, 229, 255));
                        render_mini_peak_badge(ui, "SUB", peak_out, Color32::from_rgb(255, 122, 0));
                    });
                });

                ui.add_space(2.0);

                match self.active_tab {
                    VisualizerTab::FrequencyResponse => {
                        render_bode_plot(ui, &self.audio, self.settings.cutoff_hz, self.settings.bypass);
                    }
                    VisualizerTab::Oscilloscope => {
                        render_oscilloscope(ui, &self.audio);
                    }
                }
            });

            // ==========================================
            // 4. CROSSOVER, GAIN & BASS PUNCH CONTROLS
            // ==========================================
            render_card(ui, "🎛️ CROSSOVER & POWER CONTROLS", |ui| {
                // Cutoff Frequency Slider
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Crossover Cutoff:").strong());
                    let mut cutoff = self.settings.cutoff_hz;
                    if ui.add(egui::Slider::new(&mut cutoff, 20.0..=500.0).step_by(5.0_f64).suffix(" Hz")).changed() {
                        self.settings.cutoff_hz = cutoff;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });

                // Quick Presets
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Presets:").small().color(Color32::from_rgb(130, 140, 155)));
                    for &(label, hz) in &[
                        ("40 Hz Sub", 40.0),
                        ("80 Hz THX", 80.0),
                        ("120 Hz Shakers", 120.0),
                        ("160 Hz Punch", 160.0),
                    ] {
                        let active = (self.settings.cutoff_hz - hz).abs() < 1.0;
                        if ui.selectable_label(active, label).clicked() {
                            self.settings.cutoff_hz = hz;
                            self.sync_dsp_parameters();
                            self.settings.save();
                        }
                    }
                });

                ui.separator();

                // Subwoofer Gain Trim Slider
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Subwoofer Gain Trim:").strong());
                    let mut gain = self.settings.gain_db;
                    if ui.add(egui::Slider::new(&mut gain, -12.0..=30.0).step_by(0.5_f64).suffix(" dB")).changed() {
                        self.settings.gain_db = gain;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });

                // Bass Boost & Direct Sum Switches
                ui.horizontal(|ui| {
                    let mut direct = self.settings.direct_sum;
                    if ui.checkbox(&mut direct, "Direct Summing (+6 dB Mono Voltage Boost)").on_hover_text("Sums L+R without 0.5x halving, delivering twice the voltage to drive subwoofers and chair transducers harder").changed() {
                        self.settings.direct_sum = direct;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }

                    let mut soft = self.settings.soft_clip;
                    if ui.checkbox(&mut soft, "Tape Soft-Saturator / Limiter").on_hover_text("Analog-modeled soft saturation prevents harsh DAC clipping when boosting volume heavily").changed() {
                        self.settings.soft_clip = soft;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });
            });

            // ==========================================
            // 5. ACOUSTIC ALIGNMENT & TRANSDUCER SPATIAL MODES
            // ==========================================
            render_card(ui, "⚙️ ACOUSTIC ALIGNMENT & SPATIAL HAPTICS", |ui| {
                // Stereo Transducer vs Mono Subwoofer
                let mut stereo = self.settings.stereo;
                if ui.checkbox(&mut stereo, "Stereo Dual-Transducer Mode (Independent L/R Chair Rumble)").changed() {
                    self.settings.stereo = stereo;
                    self.sync_dsp_parameters();
                    self.settings.save();
                }

                ui.label(
                    egui::RichText::new(if stereo {
                        "● Mode: Discrete Left & Right haptic channels for racing/gaming rigs (curb rumbles on left vs right)."
                    } else {
                        "● Mode: Dual-Mono summed bass feeding single or dual traditional subwoofer cabinets."
                    })
                    .small()
                    .color(Color32::from_rgb(140, 150, 165)),
                );

                ui.separator();

                // Delay Slider with Distance Equivalent
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Acoustic Delay:").strong());
                    let mut delay = self.settings.delay_ms;
                    if ui.add(egui::Slider::new(&mut delay, 0.0..=500.0).step_by(1.0_f64).suffix(" ms")).changed() {
                        self.settings.delay_ms = delay;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });

                // Real-time acoustic distance equivalent readout
                let dist_m = self.settings.delay_ms * 0.343;
                let dist_ft = dist_m * 3.28084;
                ui.label(
                    egui::RichText::new(format!("Distance Alignment: ~{dist_m:.1} meters ({dist_ft:.1} feet) physical wave travel"))
                        .small()
                        .color(Color32::from_rgb(120, 130, 145)),
                );

                ui.separator();

                // Phase Inversion & DSP Bypass
                ui.horizontal(|ui| {
                    let phase_inv = self.settings.phase_inverted;
                    let phase_label = if phase_inv { "Phase: 180° (Inverted Polarity)" } else { "Phase: 0° (Normal)" };
                    if ui.selectable_label(phase_inv, phase_label).on_hover_text("Inverts audio polarity by 180° to fix acoustic phase cancellation with main speakers").clicked() {
                        self.settings.phase_inverted = !phase_inv;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }

                    let bypass = self.settings.bypass;
                    let bypass_label = if bypass { "DSP Bypassed (Full Range)" } else { "DSP Active (LR4 Filter)" };
                    if ui.selectable_label(bypass, bypass_label).on_hover_text("Bypasses filtering for direct A/B calibration").clicked() {
                        self.settings.bypass = !bypass;
                        self.sync_dsp_parameters();
                        self.settings.save();
                    }
                });
            });
        });
    }
}

// ==========================================
// CARD CONTAINER WIDGET
// ==========================================
fn render_card<R>(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(Color32::from_rgb(17, 21, 28))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(33, 40, 52)))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(0, 229, 255)),
            );
            ui.add_space(4.0);
            add_contents(ui)
        })
        .inner
}

// ==========================================
// REAL-TIME FREQUENCY RESPONSE (BODE PLOT)
// ==========================================
fn render_bode_plot(ui: &mut egui::Ui, audio: &AudioEngine, cutoff_hz: f32, bypass: bool) {
    let desired_size = Vec2::new(ui.available_width(), 130.0);
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Deep grid background
    painter.rect_filled(rect, 6.0, Color32::from_rgb(10, 13, 17));
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, Color32::from_rgb(28, 35, 48)));

    let plot = rect.shrink2(Vec2::new(12.0, 12.0));
    let (low_log, high_log) = (20f32.log10(), 1000f32.log10());

    let x_of = |hz: f32| plot.left() + (hz.log10() - low_log) / (high_log - low_log) * plot.width();
    let min_db = -36.0f32;
    let max_db = 18.0f32;
    let y_of = |db: f32| {
        let normalized = (db.clamp(min_db, max_db) - min_db) / (max_db - min_db);
        plot.bottom() - normalized * plot.height()
    };

    // Horizontal dB grid lines
    for &db in &[-24.0, -12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = y_of(db);
        let color = if db == 0.0 {
            Color32::from_rgb(60, 75, 95)
        } else {
            Color32::from_rgb(25, 32, 42)
        };
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(0.5_f32, color));

        painter.text(
            Pos2::new(plot.left() + 4.0, y - 6.0),
            egui::Align2::LEFT_CENTER,
            format!("{db:+.0} dB"),
            egui::FontId::proportional(9.0),
            Color32::from_rgb(90, 100, 115),
        );
    }

    // Vertical frequency grid lines
    for &hz in &[30.0, 50.0, 80.0, 120.0, 200.0, 500.0] {
        let x = x_of(hz);
        painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(0.5_f32, Color32::from_rgb(25, 32, 42)));
        painter.text(
            Pos2::new(x, plot.bottom() + 2.0),
            egui::Align2::CENTER_TOP,
            format!("{hz:.0}Hz"),
            egui::FontId::proportional(9.0),
            Color32::from_rgb(90, 100, 115),
        );
    }

    // Linkwitz-Riley 4th-order response curve
    let dsp_guard = audio.dsp.lock().unwrap();
    let steps = 180;
    let points: Vec<Pos2> = (0..=steps)
        .map(|step| {
            let t = step as f32 / steps as f32;
            let hz = 10f32.powf(low_log + t * (high_log - low_log));
            let db = dsp_guard.lr4_gain_db_at(hz as f64) as f32;
            Pos2::new(plot.left() + t * plot.width(), y_of(db))
        })
        .collect();

    if points.len() >= 2 {
        // Shaded area under curve
        let mut poly = points.clone();
        poly.push(Pos2::new(plot.right(), plot.bottom()));
        poly.push(Pos2::new(plot.left(), plot.bottom()));
        painter.add(Shape::convex_polygon(
            poly,
            Color32::from_rgba_unmultiplied(255, 122, 0, 22),
            Stroke::NONE,
        ));

        // Active Curve Line
        let line_color = if bypass {
            Color32::from_rgb(100, 115, 130)
        } else {
            Color32::from_rgb(255, 122, 0)
        };
        painter.add(Shape::line(points, Stroke::new(2.2_f32, line_color)));
    }

    // Cutoff frequency marker circle
    let cut_x = x_of(cutoff_hz);
    let cut_y = y_of(dsp_guard.lr4_gain_db_at(cutoff_hz as f64) as f32);
    painter.circle_filled(Pos2::new(cut_x, cut_y), 4.0, Color32::from_rgb(0, 229, 255));
    painter.line_segment(
        [Pos2::new(cut_x, cut_y), Pos2::new(cut_x, plot.bottom())],
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0, 229, 255, 120)),
    );
}

// ==========================================
// REAL-TIME DUAL OSCILLOSCOPE
// ==========================================
fn render_oscilloscope(ui: &mut egui::Ui, audio: &AudioEngine) {
    let desired_size = Vec2::new(ui.available_width(), 130.0);
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Dark oscilloscope background
    painter.rect_filled(rect, 6.0, Color32::from_rgb(10, 13, 17));
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, Color32::from_rgb(28, 35, 48)));

    let mid_y = rect.center().y;
    painter.line_segment(
        [Pos2::new(rect.left(), mid_y), Pos2::new(rect.right(), mid_y)],
        Stroke::new(0.5_f32, Color32::from_rgb(35, 45, 60)),
    );

    let in_samples = audio.tap.input_wave.lock().unwrap().clone();
    let bass_samples = audio.tap.bass_wave.lock().unwrap().clone();

    let n = in_samples.len().min(bass_samples.len()).max(2);
    let step_x = rect.width() / (n - 1) as f32;
    let amp = rect.height() * 0.44;

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
        painter.add(Shape::line(
            in_points,
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0, 229, 255, 120)),
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
        painter.add(Shape::line(
            bass_points,
            Stroke::new(2.4_f32, Color32::from_rgb(255, 122, 0)),
        ));
    }

    // Legend
    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "■ Input Full-Range Audio",
        egui::FontId::proportional(10.0),
        Color32::from_rgb(0, 229, 255),
    );
    painter.text(
        Pos2::new(rect.left() + 150.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "■ Filtered Sub-Bass / Shakers",
        egui::FontId::proportional(10.0),
        Color32::from_rgb(255, 122, 0),
    );
}

// ==========================================
// MINI HARDWARE PEAK LEVEL BADGE
// ==========================================
fn render_mini_peak_badge(ui: &mut egui::Ui, name: &str, peak: f32, color: Color32) {
    let db = if peak > 0.0001 {
        20.0 * peak.log10()
    } else {
        -48.0
    };

    let meter_w = 40.0;
    let meter_h = 8.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(meter_w + 30.0, 14.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.text(
        Pos2::new(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(10.0),
        Color32::from_rgb(160, 170, 185),
    );

    let bar_rect = Rect::from_min_size(Pos2::new(rect.left() + 26.0, rect.center().y - meter_h * 0.5), Vec2::new(meter_w, meter_h));
    painter.rect_filled(bar_rect, 2.0, Color32::from_rgb(15, 19, 25));

    let frac = ((db + 36.0) / 36.0).clamp(0.0, 1.0);
    if frac > 0.02 {
        let fill_w = bar_rect.width() * frac;
        let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, meter_h));
        let fill_color = if db > -0.5 {
            Color32::from_rgb(255, 45, 65) // Clipping
        } else {
            color
        };
        painter.rect_filled(fill_rect, 2.0, fill_color);
    }
}
