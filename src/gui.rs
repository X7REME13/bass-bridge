use crate::audio::AudioEngine;
use crate::dsp::{compute_spectrum_db, lr4_response_db, make_lr4_stage, FFT_SIZE};
use crate::settings::Settings;
use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Shape, Stroke, Vec2};
use std::time::Duration;

const BG_APP: Color32 = Color32::from_rgb(19, 20, 23);
const BG_CARD: Color32 = Color32::from_rgb(27, 29, 34);
const BG_PLOT: Color32 = Color32::from_rgb(13, 15, 18);
const BORDER: Color32 = Color32::from_rgb(42, 45, 53);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(232, 235, 240);
const TEXT_MUTED: Color32 = Color32::from_rgb(138, 146, 160);
const TEXT_DIM: Color32 = Color32::from_rgb(92, 99, 112);

const ACCENT_AMBER: Color32 = Color32::from_rgb(235, 134, 52);
const ACCENT_BLUE: Color32 = Color32::from_rgb(104, 138, 178);
const STATUS_GREEN: Color32 = Color32::from_rgb(63, 185, 80);
const STATUS_RED: Color32 = Color32::from_rgb(229, 83, 75);
const STATUS_WARN: Color32 = Color32::from_rgb(218, 156, 56);

#[derive(PartialEq, Clone, Copy)]
enum VisualizerTab {
    Spectrum,
    Waveform,
}

pub struct BassBridgeApp {
    settings: Settings,
    audio: AudioEngine,
    output_devices: Vec<String>,
    input_devices: Vec<String>,
    default_output_name: Option<String>,
    last_error: Option<String>,
    active_tab: VisualizerTab,
    // Persistent visualizer buffers (zero per-frame heap allocations)
    in_wave: Vec<f32>,
    bass_wave: Vec<f32>,
    in_fft_db: Vec<f32>,
    bass_fft_db: Vec<f32>,
    smooth_in_db: Vec<f32>,
    smooth_bass_db: Vec<f32>,
    meter_in: f32,
    meter_out: f32,
}

impl BassBridgeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::apply_theme(&cc.egui_ctx);

        let settings = Settings::load();
        let output_devices = AudioEngine::list_output_devices();
        let input_devices = AudioEngine::list_input_devices();
        let default_output_name = AudioEngine::default_output_device_name();
        let audio = AudioEngine::new();

        let half_fft = FFT_SIZE / 2;
        let mut app = Self {
            settings,
            audio,
            output_devices,
            input_devices,
            default_output_name,
            last_error: None,
            active_tab: VisualizerTab::Spectrum,
            in_wave: vec![0.0; FFT_SIZE],
            bass_wave: vec![0.0; FFT_SIZE],
            in_fft_db: vec![-96.0; half_fft],
            bass_fft_db: vec![-96.0; half_fft],
            smooth_in_db: vec![-96.0; half_fft],
            smooth_bass_db: vec![-96.0; half_fft],
            meter_in: 0.0,
            meter_out: 0.0,
        };

        if app.settings.enabled {
            if let Err(e) = app.audio.start(&app.settings) {
                app.last_error = Some(e);
                app.settings.enabled = false;
            }
        }

        app
    }

    fn apply_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.dark_mode = true;
        visuals.panel_fill = BG_APP;
        visuals.window_fill = BG_APP;
        visuals.extreme_bg_color = BG_PLOT;
        visuals.faint_bg_color = Color32::from_rgb(23, 25, 30);
        visuals.override_text_color = Some(TEXT_PRIMARY);
        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(235, 134, 52, 48);
        visuals.selection.stroke = Stroke::new(1.0, ACCENT_AMBER);

        visuals.widgets.noninteractive.bg_fill = BG_CARD;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(5.0);

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(33, 36, 43);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(52, 56, 66));
        visuals.widgets.inactive.rounding = egui::Rounding::same(5.0);

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 46, 56);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_AMBER);
        visuals.widgets.hovered.rounding = egui::Rounding::same(5.0);

        visuals.widgets.active.bg_fill = Color32::from_rgb(48, 52, 64);
        visuals.widgets.active.bg_stroke = Stroke::new(1.2, ACCENT_AMBER);
        visuals.widgets.active.rounding = egui::Rounding::same(5.0);

        ctx.set_visuals(visuals);
    }

    fn refresh_devices(&mut self) {
        self.output_devices = AudioEngine::list_output_devices();
        self.input_devices = AudioEngine::list_input_devices();
        self.default_output_name = AudioEngine::default_output_device_name();
    }

    fn restart_if_running(&mut self) {
        if self.audio.is_running() {
            match self.audio.start(&self.settings) {
                Ok(()) => self.last_error = None,
                Err(e) => {
                    self.last_error = Some(e);
                    self.settings.enabled = false;
                }
            }
        }
    }

    fn sync_dsp_parameters(&self) {
        self.audio.sync_settings(&self.settings);
    }

    fn has_same_device_loopback(&self) -> Option<String> {
        let in_is_loopback = match self.settings.input_device.as_deref() {
            None => true,
            Some(name) => self.output_devices.iter().any(|d| d == name),
        };
        if !in_is_loopback {
            return None;
        }
        let resolved_in = self
            .settings
            .input_device
            .as_deref()
            .or(self.default_output_name.as_deref())?;
        let resolved_out = self
            .settings
            .output_device
            .as_deref()
            .or(self.default_output_name.as_deref())?;
        if resolved_in == resolved_out {
            Some(resolved_in.to_string())
        } else {
            None
        }
    }

    fn update_visualizer_state(&mut self, dt: f32) {
        if !self.audio.is_running() {
            self.meter_in = 0.0;
            self.meter_out = 0.0;
            self.in_wave.fill(0.0);
            self.bass_wave.fill(0.0);
            self.smooth_in_db.fill(-96.0);
            self.smooth_bass_db.fill(-96.0);
            return;
        }

        let (pk_in, pk_out) = self.audio.tap.take_peaks();
        let decay = (-dt / 0.25).exp();
        self.meter_in = pk_in.max(self.meter_in * decay);
        self.meter_out = pk_out.max(self.meter_out * decay);

        self.audio.tap.snapshot_input(&mut self.in_wave);
        self.audio.tap.snapshot_bass(&mut self.bass_wave);

        if self.active_tab == VisualizerTab::Spectrum {
            compute_spectrum_db(&self.in_wave, &mut self.in_fft_db);
            compute_spectrum_db(&self.bass_wave, &mut self.bass_fft_db);

            let alpha_down = 1.0 - (-dt / 0.18).exp();
            for (cur, &raw) in self.smooth_in_db.iter_mut().zip(self.in_fft_db.iter()) {
                let alpha = if raw > *cur { 0.65 } else { alpha_down };
                *cur += alpha * (raw - *cur);
            }
            for (cur, &raw) in self.smooth_bass_db.iter_mut().zip(self.bass_fft_db.iter()) {
                let alpha = if raw > *cur { 0.65 } else { alpha_down };
                *cur += alpha * (raw - *cur);
            }
        }
    }
}

impl eframe::App for BassBridgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let running = self.audio.is_running();
        if running {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1);
        self.update_visualizer_state(dt);

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

                    // ==========================================
                    // 1. HEADER BAR
                    // ==========================================
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("BassBridge")
                                .size(18.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        egui::Frame::none()
                            .fill(Color32::from_rgb(33, 36, 43))
                            .stroke(Stroke::new(1.0, BORDER))
                            .rounding(egui::Rounding::same(4.0))
                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                                        .size(11.0)
                                        .monospace()
                                        .color(TEXT_MUTED),
                                );
                            });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_label = if running { "Stop" } else { "Start" };
                            let btn_fill = if running {
                                Color32::from_rgb(175, 58, 52)
                            } else {
                                Color32::from_rgb(194, 106, 38)
                            };
                            let btn = egui::Button::new(
                                egui::RichText::new(btn_label)
                                    .size(12.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(btn_fill)
                            .min_size(Vec2::new(68.0, 26.0));

                            if ui.add(btn).clicked() {
                                if running {
                                    self.audio.stop();
                                    self.settings.enabled = false;
                                    self.settings.save();
                                    self.last_error = None;
                                } else {
                                    match self.audio.start(&self.settings) {
                                        Ok(()) => {
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

                            ui.add_space(6.0);

                            let status_text = if running {
                                let (ir, or) = self.audio.active_rates();
                                if ir > 0 && or > 0 {
                                    if ir == or {
                                        format!("Running · {} kHz", format_khz(or))
                                    } else {
                                        format!(
                                            "Running · {} -> {} kHz",
                                            format_khz(ir),
                                            format_khz(or)
                                        )
                                    }
                                } else {
                                    "Running".to_string()
                                }
                            } else {
                                "Stopped".to_string()
                            };
                            let dot_color = if running { STATUS_GREEN } else { TEXT_DIM };

                            ui.label(
                                egui::RichText::new(status_text)
                                    .size(11.5)
                                    .color(if running { TEXT_PRIMARY } else { TEXT_MUTED }),
                            );
                            let (dot_rect, _) = ui.allocate_exact_size(
                                Vec2::new(8.0, 8.0),
                                egui::Sense::hover(),
                            );
                            ui.painter()
                                .circle_filled(dot_rect.center(), 3.5, dot_color);
                        });
                    });

                    // ==========================================
                    // 2. ROUTING CARD
                    // ==========================================
                    render_card(ui, "Routing", |ui| {
                        let combo_w = (ui.available_width() - 150.0).max(180.0);

                        egui::Grid::new("routing-grid")
                            .num_columns(2)
                            .spacing([10.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Input").color(TEXT_MUTED));
                                ui.horizontal(|ui| {
                                    let default_in_label = match &self.default_output_name {
                                        Some(name) => format!("Default ({name})"),
                                        None => "System Default Playback".to_string(),
                                    };
                                    let selected_in = self
                                        .settings
                                        .input_device
                                        .clone()
                                        .unwrap_or_else(|| default_in_label.clone());

                                    let mut changed_in = false;
                                    egui::ComboBox::from_id_salt("input-device-combo")
                                        .selected_text(&selected_in)
                                        .width(combo_w)
                                        .show_ui(ui, |ui| {
                                            if ui
                                                .selectable_label(
                                                    self.settings.input_device.is_none(),
                                                    &default_in_label,
                                                )
                                                .clicked()
                                            {
                                                self.settings.input_device = None;
                                                changed_in = true;
                                            }
                                            for dev in &self.output_devices {
                                                let active = self.settings.input_device.as_deref()
                                                    == Some(dev.as_str());
                                                if ui
                                                    .selectable_label(
                                                        active,
                                                        format!("{dev} (Loopback)"),
                                                    )
                                                    .clicked()
                                                {
                                                    self.settings.input_device = Some(dev.clone());
                                                    changed_in = true;
                                                }
                                            }
                                            for dev in &self.input_devices {
                                                if !self.output_devices.contains(dev) {
                                                    let active =
                                                        self.settings.input_device.as_deref()
                                                            == Some(dev.as_str());
                                                    if ui
                                                        .selectable_label(
                                                            active,
                                                            format!("{dev} (Mic / Line)"),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.settings.input_device =
                                                            Some(dev.clone());
                                                        changed_in = true;
                                                    }
                                                }
                                            }
                                        });

                                    if changed_in {
                                        self.settings.save();
                                        self.restart_if_running();
                                    }

                                    if ui
                                        .button("Refresh")
                                        .on_hover_text("Rescan audio endpoints")
                                        .clicked()
                                    {
                                        self.refresh_devices();
                                    }
                                });
                                ui.end_row();

                                ui.label(egui::RichText::new("Output").color(TEXT_MUTED));
                                ui.horizontal(|ui| {
                                    let default_out_label = match &self.default_output_name {
                                        Some(name) => format!("Default ({name})"),
                                        None => "System Default Output".to_string(),
                                    };
                                    let selected_out = self
                                        .settings
                                        .output_device
                                        .clone()
                                        .unwrap_or_else(|| default_out_label.clone());

                                    let mut changed_out = false;
                                    egui::ComboBox::from_id_salt("output-device-combo")
                                        .selected_text(&selected_out)
                                        .width(combo_w)
                                        .show_ui(ui, |ui| {
                                            if ui
                                                .selectable_label(
                                                    self.settings.output_device.is_none(),
                                                    &default_out_label,
                                                )
                                                .clicked()
                                            {
                                                self.settings.output_device = None;
                                                changed_out = true;
                                            }
                                            for dev in &self.output_devices {
                                                let active = self.settings.output_device.as_deref()
                                                    == Some(dev.as_str());
                                                if ui.selectable_label(active, dev).clicked() {
                                                    self.settings.output_device = Some(dev.clone());
                                                    changed_out = true;
                                                }
                                            }
                                        });

                                    if changed_out {
                                        self.settings.save();
                                        self.restart_if_running();
                                    }
                                });
                                ui.end_row();
                            });

                        let runtime_err = self
                            .last_error
                            .clone()
                            .or_else(|| self.audio.stream_error());

                        if let Some(err) = runtime_err {
                            render_notice(ui, &err, STATUS_RED);
                        } else if let Some(dev_name) = self.has_same_device_loopback() {
                            render_notice(
                                ui,
                                &format!(
                                    "Output matches loopback input ({dev_name}). Select a secondary output device before starting."
                                ),
                                STATUS_WARN,
                            );
                        }
                    });

                    // ==========================================
                    // 3. MONITOR CARD (SPECTRUM + WAVEFORM)
                    // ==========================================
                    render_card(ui, "Monitor", |ui| {
                        ui.horizontal(|ui| {
                            if seg_button(
                                ui,
                                self.active_tab == VisualizerTab::Spectrum,
                                "Spectrum",
                            )
                            .clicked()
                            {
                                self.active_tab = VisualizerTab::Spectrum;
                            }
                            if seg_button(
                                ui,
                                self.active_tab == VisualizerTab::Waveform,
                                "Waveform",
                            )
                            .clicked()
                            {
                                self.active_tab = VisualizerTab::Waveform;
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    render_level_meter(
                                        ui,
                                        "OUT",
                                        self.meter_out,
                                        ACCENT_AMBER,
                                    );
                                    ui.add_space(6.0);
                                    render_level_meter(ui, "IN", self.meter_in, ACCENT_BLUE);
                                },
                            );
                        });

                        ui.add_space(2.0);

                        let (in_rate, out_rate) = self.audio.active_rates();
                        let sr_in = if in_rate > 0 { in_rate as f32 } else { 48000.0 };
                        let sr_out = if out_rate > 0 { out_rate as f32 } else { 48000.0 };

                        match self.active_tab {
                            VisualizerTab::Spectrum => {
                                let (new_cutoff, finalize_drag) = render_spectrum_plot(
                                    ui,
                                    &self.smooth_in_db,
                                    &self.smooth_bass_db,
                                    sr_in,
                                    sr_out,
                                    &self.settings,
                                    running,
                                );
                                if let Some(hz) = new_cutoff {
                                    self.settings.cutoff_hz = hz;
                                    self.sync_dsp_parameters();
                                }
                                if finalize_drag {
                                    self.settings.save();
                                }
                            }
                            VisualizerTab::Waveform => {
                                render_oscilloscope(
                                    ui,
                                    &self.in_wave,
                                    &self.bass_wave,
                                    self.settings.bypass,
                                    running,
                                );
                            }
                        }
                    });

                    // ==========================================
                    // 4. CROSSOVER & OUTPUT CARD
                    // ==========================================
                    render_card(ui, "Crossover", |ui| {
                        let slider_w = (ui.available_width() - 250.0).max(120.0);
                        let mut needs_sync = false;
                        let mut needs_save = false;

                        egui::Grid::new("crossover-grid")
                            .num_columns(3)
                            .spacing([10.0, 8.0])
                            .show(ui, |ui| {
                                // Cutoff
                                ui.label(egui::RichText::new("Cutoff").color(TEXT_MUTED));
                                let mut cutoff = self.settings.cutoff_hz;
                                let r_cut = ui.add_sized(
                                    [slider_w, 20.0],
                                    egui::Slider::new(&mut cutoff, 20.0..=300.0)
                                        .step_by(1.0)
                                        .suffix(" Hz"),
                                );
                                if r_cut.changed() {
                                    self.settings.cutoff_hz = cutoff;
                                    needs_sync = true;
                                    if !r_cut.dragged() {
                                        needs_save = true;
                                    }
                                }
                                if r_cut.drag_stopped() {
                                    needs_save = true;
                                }

                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    for &hz in &[40.0f32, 80.0, 120.0, 160.0] {
                                        let active = (self.settings.cutoff_hz - hz).abs() < 0.5;
                                        if seg_button(ui, active, &format!("{hz:.0}")).clicked() {
                                            self.settings.cutoff_hz = hz;
                                            needs_sync = true;
                                            needs_save = true;
                                        }
                                    }
                                });
                                ui.end_row();

                                // Gain
                                ui.label(egui::RichText::new("Gain").color(TEXT_MUTED));
                                let mut gain = self.settings.gain_db;
                                let r_gain = ui.add_sized(
                                    [slider_w, 20.0],
                                    egui::Slider::new(&mut gain, -12.0..=30.0)
                                        .step_by(0.5)
                                        .suffix(" dB"),
                                );
                                if r_gain.changed() {
                                    self.settings.gain_db = gain;
                                    needs_sync = true;
                                    if !r_gain.dragged() {
                                        needs_save = true;
                                    }
                                }
                                if r_gain.drag_stopped() {
                                    needs_save = true;
                                }
                                let gain_hint = if self.settings.direct_sum && !self.settings.stereo
                                {
                                    "Effective w/ sum: +6 dB"
                                } else {
                                    "24 dB/oct LR4"
                                };
                                ui.label(egui::RichText::new(gain_hint).size(11.0).color(TEXT_DIM));
                                ui.end_row();

                                // Delay
                                ui.label(egui::RichText::new("Delay").color(TEXT_MUTED));
                                let mut delay = self.settings.delay_ms;
                                let r_delay = ui.add_sized(
                                    [slider_w, 20.0],
                                    egui::Slider::new(&mut delay, 0.0..=500.0)
                                        .step_by(0.5)
                                        .suffix(" ms"),
                                );
                                if r_delay.changed() {
                                    self.settings.delay_ms = delay;
                                    needs_sync = true;
                                    if !r_delay.dragged() {
                                        needs_save = true;
                                    }
                                }
                                if r_delay.drag_stopped() {
                                    needs_save = true;
                                }
                                let dist_m = self.settings.delay_ms * 0.343;
                                ui.label(
                                    egui::RichText::new(format!("{dist_m:.2} m"))
                                        .size(11.5)
                                        .monospace()
                                        .color(TEXT_MUTED),
                                );
                                ui.end_row();
                            });

                        ui.separator();

                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);

                            if seg_button(ui, !self.settings.stereo, "Mono").clicked()
                                && self.settings.stereo
                            {
                                self.settings.stereo = false;
                                needs_sync = true;
                                needs_save = true;
                            }
                            if seg_button(ui, self.settings.stereo, "Stereo").clicked()
                                && !self.settings.stereo
                            {
                                self.settings.stereo = true;
                                needs_sync = true;
                                needs_save = true;
                            }

                            let sum_resp = ui.add_enabled(
                                !self.settings.stereo,
                                egui::Button::new(
                                    egui::RichText::new("Direct Sum (+6 dB)").size(11.5).color(
                                        if !self.settings.stereo && self.settings.direct_sum {
                                            TEXT_PRIMARY
                                        } else {
                                            TEXT_MUTED
                                        },
                                    ),
                                )
                                .fill(if !self.settings.stereo && self.settings.direct_sum {
                                    Color32::from_rgba_unmultiplied(235, 134, 52, 42)
                                } else {
                                    Color32::from_rgb(33, 36, 43)
                                })
                                .stroke(Stroke::new(
                                    1.0,
                                    if !self.settings.stereo && self.settings.direct_sum {
                                        ACCENT_AMBER
                                    } else {
                                        BORDER
                                    },
                                )),
                            );
                            if sum_resp
                                .on_hover_text("Sum L+R at full unity gain in Mono mode")
                                .clicked()
                            {
                                self.settings.direct_sum = !self.settings.direct_sum;
                                needs_sync = true;
                                needs_save = true;
                            }

                            if seg_button(ui, self.settings.limiter, "Limiter")
                                .on_hover_text("Envelope peak limiter with C1 soft ceiling at -0.7 dBFS")
                                .clicked()
                            {
                                self.settings.limiter = !self.settings.limiter;
                                needs_sync = true;
                                needs_save = true;
                            }

                            if seg_button(ui, self.settings.phase_inverted, "Invert Phase")
                                .on_hover_text("Flip output polarity by 180 degrees")
                                .clicked()
                            {
                                self.settings.phase_inverted = !self.settings.phase_inverted;
                                needs_sync = true;
                                needs_save = true;
                            }

                            if seg_button(ui, self.settings.bypass, "Bypass")
                                .on_hover_text("Bypass LR4 filter for direct A/B comparison")
                                .clicked()
                            {
                                self.settings.bypass = !self.settings.bypass;
                                needs_sync = true;
                                needs_save = true;
                            }
                        });

                        if needs_sync {
                            self.sync_dsp_parameters();
                        }
                        if needs_save {
                            self.settings.save();
                        }
                    });
                });
        });
    }
}

fn format_khz(rate_hz: u32) -> String {
    if rate_hz % 1000 == 0 {
        format!("{}", rate_hz / 1000)
    } else {
        format!("{:.1}", rate_hz as f32 / 1000.0)
    }
}

fn seg_button(ui: &mut egui::Ui, active: bool, label: &str) -> egui::Response {
    let fill = if active {
        Color32::from_rgba_unmultiplied(235, 134, 52, 42)
    } else {
        Color32::from_rgb(33, 36, 43)
    };
    let stroke_color = if active { ACCENT_AMBER } else { BORDER };
    let text_color = if active { TEXT_PRIMARY } else { TEXT_MUTED };
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(11.5).color(text_color))
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke_color))
            .min_size(Vec2::new(28.0, 22.0)),
    )
}

fn render_notice(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add_space(4.0);
    let bg = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 22);
    egui::Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0, color))
        .rounding(egui::Rounding::same(4.0))
        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.5).color(TEXT_PRIMARY));
        });
}

fn render_card<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::none()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(11.0)
                    .strong()
                    .color(TEXT_MUTED),
            );
            ui.add_space(2.0);
            add_contents(ui)
        })
        .inner
}

// ==========================================
// REAL-TIME FFT SPECTRUM + LR4 CURVE
// ==========================================
fn sample_spectrum_band_db(bins: &[f32], sample_rate: f32, hz_lo: f32, hz_center: f32, hz_hi: f32) -> f32 {
    if bins.is_empty() {
        return -96.0;
    }
    let max_idx = bins.len() - 1;
    let df = sample_rate.max(8000.0) / FFT_SIZE as f32;
    let b_lo = (hz_lo / df).clamp(1.0, max_idx as f32);
    let b_hi = (hz_hi / df).clamp(1.0, max_idx as f32);

    if (b_hi - b_lo) <= 1.0 {
        let bf = (hz_center / df).clamp(1.0, max_idx as f32);
        let i0 = bf.floor() as usize;
        let i1 = (i0 + 1).min(max_idx);
        let frac = bf - i0 as f32;
        bins[i0] + (bins[i1] - bins[i0]) * frac
    } else {
        let i_start = b_lo.floor() as usize;
        let i_end = (b_hi.ceil() as usize).min(max_idx);
        let mut peak = -120.0f32;
        for &val in &bins[i_start..=i_end] {
            if val > peak {
                peak = val;
            }
        }
        peak
    }
}

fn build_gradient_area_mesh(
    points: &[Pos2],
    bottom_y: f32,
    top_color: Color32,
    bottom_color: Color32,
) -> Mesh {
    let mut mesh = Mesh::default();
    if points.len() < 2 {
        return mesh;
    }
    for (i, &pt) in points.iter().enumerate() {
        mesh.colored_vertex(pt, top_color);
        mesh.colored_vertex(Pos2::new(pt.x, bottom_y), bottom_color);
        if i > 0 {
            let base = (i as u32 - 1) * 2;
            mesh.add_triangle(base, base + 2, base + 1);
            mesh.add_triangle(base + 1, base + 2, base + 3);
        }
    }
    mesh
}

fn render_spectrum_plot(
    ui: &mut egui::Ui,
    smooth_in_db: &[f32],
    smooth_bass_db: &[f32],
    sample_rate_in: f32,
    sample_rate_out: f32,
    settings: &Settings,
    running: bool,
) -> (Option<f32>, bool) {
    let desired_size = Vec2::new(ui.available_width(), 155.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 5.0, BG_PLOT);
    painter.rect_stroke(rect, 5.0, Stroke::new(1.0, BORDER));

    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 34.0, rect.top() + 10.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - 18.0),
    );
    let (low_log, high_log) = (20f32.log10(), 1000f32.log10());
    let x_of = |hz: f32| plot.left() + (hz.log10() - low_log) / (high_log - low_log) * plot.width();

    let min_db = -54.0f32;
    let max_db = 18.0f32;
    let y_of = |db: f32| {
        let norm = (db.clamp(min_db, max_db) - min_db) / (max_db - min_db);
        plot.bottom() - norm * plot.height()
    };

    // Horizontal dB grid
    for &db in &[0.0f32, -12.0, -24.0, -36.0, -48.0] {
        let y = y_of(db);
        let grid_color = if db == 0.0 {
            Color32::from_rgb(56, 62, 74)
        } else {
            Color32::from_rgb(26, 30, 37)
        };
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(0.7, grid_color),
        );
        painter.text(
            Pos2::new(plot.left() - 4.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{db:.0}"),
            egui::FontId::monospace(9.5),
            TEXT_DIM,
        );
    }

    // Vertical Hz grid
    for &hz in &[30.0f32, 50.0, 80.0, 120.0, 200.0, 500.0] {
        let x = x_of(hz);
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(0.7, Color32::from_rgb(26, 30, 37)),
        );
        painter.text(
            Pos2::new(x, plot.bottom() + 4.0),
            egui::Align2::CENTER_TOP,
            format!("{hz:.0}"),
            egui::FontId::monospace(9.5),
            TEXT_DIM,
        );
    }

    let steps = 160usize;
    let freqs: Vec<f32> = (0..=steps)
        .map(|s| {
            let t = s as f32 / steps as f32;
            10f32.powf(low_log + t * (high_log - low_log))
        })
        .collect();

    if running {
        let mut in_pts = Vec::with_capacity(freqs.len());
        let mut bass_pts = Vec::with_capacity(freqs.len());

        for i in 0..freqs.len() {
            let f_curr = freqs[i];
            let f_prev = if i > 0 { freqs[i - 1] } else { f_curr * 0.95 };
            let f_next = if i + 1 < freqs.len() {
                freqs[i + 1]
            } else {
                f_curr * 1.05
            };
            let lo = 0.5 * (f_prev + f_curr);
            let hi = 0.5 * (f_curr + f_next);
            let x = x_of(f_curr);

            let db_in = sample_spectrum_band_db(smooth_in_db, sample_rate_in, lo, f_curr, hi);
            let db_bass = sample_spectrum_band_db(smooth_bass_db, sample_rate_out, lo, f_curr, hi);

            in_pts.push(Pos2::new(x, y_of(db_in)));
            bass_pts.push(Pos2::new(x, y_of(db_bass)));
        }

        let in_mesh = build_gradient_area_mesh(
            &in_pts,
            plot.bottom(),
            Color32::from_rgba_unmultiplied(104, 138, 178, 52),
            Color32::from_rgba_unmultiplied(104, 138, 178, 4),
        );
        painter.add(Shape::mesh(in_mesh));
        painter.add(Shape::line(
            in_pts,
            Stroke::new(1.1, Color32::from_rgba_unmultiplied(104, 138, 178, 175)),
        ));

        let bass_mesh = build_gradient_area_mesh(
            &bass_pts,
            plot.bottom(),
            Color32::from_rgba_unmultiplied(235, 134, 52, 80),
            Color32::from_rgba_unmultiplied(235, 134, 52, 8),
        );
        painter.add(Shape::mesh(bass_mesh));
        painter.add(Shape::line(bass_pts, Stroke::new(1.8, ACCENT_AMBER)));
    }

    // Analytical LR4 crossover response overlay (zero audio thread locking)
    let stage = make_lr4_stage(settings.cutoff_hz as f64, sample_rate_out as f64);
    let gain_f64 = settings.gain_db as f64;
    let lr4_pts: Vec<Pos2> = freqs
        .iter()
        .map(|&hz| {
            let db = if settings.bypass {
                gain_f64
            } else {
                2.0 * stage.gain_db_at(hz as f64) + gain_f64
            } as f32;
            Pos2::new(x_of(hz), y_of(db))
        })
        .collect();

    let curve_color = if settings.bypass {
        TEXT_MUTED
    } else {
        Color32::from_rgb(245, 200, 145)
    };
    painter.add(Shape::line(lr4_pts, Stroke::new(1.5, curve_color)));

    // Cutoff marker & interactive handle
    let cut_hz = settings.cutoff_hz.clamp(20.0, 1000.0);
    let cut_x = x_of(cut_hz);
    let cut_db = lr4_response_db(
        cut_hz as f64,
        gain_f64,
        settings.bypass,
        sample_rate_out as f64,
        cut_hz as f64,
    ) as f32;
    let cut_y = y_of(cut_db);

    painter.line_segment(
        [Pos2::new(cut_x, plot.top()), Pos2::new(cut_x, plot.bottom())],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(235, 134, 52, 110)),
    );
    painter.circle_filled(Pos2::new(cut_x, cut_y), 4.5, ACCENT_AMBER);
    painter.circle_stroke(
        Pos2::new(cut_x, cut_y),
        4.5,
        Stroke::new(1.0, Color32::WHITE),
    );

    if running {
        painter.text(
            Pos2::new(plot.right() - 2.0, plot.top() + 2.0),
            egui::Align2::RIGHT_TOP,
            format!("{cut_hz:.0} Hz"),
            egui::FontId::monospace(10.0),
            TEXT_MUTED,
        );
    } else {
        painter.text(
            Pos2::new(plot.right() - 2.0, plot.top() + 2.0),
            egui::Align2::RIGHT_TOP,
            "Idle",
            egui::FontId::monospace(10.0),
            TEXT_DIM,
        );
    }

    let mut dragged_cutoff = None;
    if response.dragged() || response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let frac = ((pos.x - plot.left()) / plot.width().max(1.0)).clamp(0.0, 1.0);
            let hz = 10f32
                .powf(low_log + frac * (high_log - low_log))
                .clamp(20.0, 300.0);
            dragged_cutoff = Some(hz.round());
        }
    }
    let finalize = response.drag_stopped() || response.clicked();
    (dragged_cutoff, finalize)
}

// ==========================================
// TRIGGERED DUAL OSCILLOSCOPE
// ==========================================
fn find_rising_trigger(wave: &[f32], window: usize) -> Option<usize> {
    if wave.len() <= window + 2 {
        return None;
    }
    let mut peak = 0.0f32;
    for &s in wave {
        peak = peak.max(s.abs());
    }
    if peak < 0.002 {
        return None;
    }

    let max_start = wave.len() - window;
    for i in (1..max_start).rev() {
        if wave[i - 1] <= 0.0 && wave[i] > 0.0 && wave[i + 1] >= wave[i] {
            return Some(i);
        }
    }
    None
}

fn render_oscilloscope(
    ui: &mut egui::Ui,
    in_wave: &[f32],
    bass_wave: &[f32],
    bypass: bool,
    running: bool,
) {
    let desired_size = Vec2::new(ui.available_width(), 155.0);
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 5.0, BG_PLOT);
    painter.rect_stroke(rect, 5.0, Stroke::new(1.0, BORDER));

    let mid_y = rect.center().y;
    painter.line_segment(
        [Pos2::new(rect.left() + 1.0, mid_y), Pos2::new(rect.right() - 1.0, mid_y)],
        Stroke::new(0.8, Color32::from_rgb(35, 40, 50)),
    );

    let window = 1600usize.min(in_wave.len()).min(bass_wave.len());
    if running && window >= 8 {
        let trig = if !bypass {
            find_rising_trigger(bass_wave, window)
                .or_else(|| find_rising_trigger(in_wave, window))
        } else {
            find_rising_trigger(in_wave, window)
        }
        .unwrap_or(in_wave.len().saturating_sub(window));

        let in_slice = &in_wave[trig..trig + window];
        let bass_slice = &bass_wave[trig..trig + window];

        let stride = (window / 400).max(1);
        let draw_count = window / stride;
        let step_x = rect.width() / (draw_count.saturating_sub(1).max(1)) as f32;
        let amp = rect.height() * 0.43;

        let in_points: Vec<Pos2> = (0..draw_count)
            .map(|i| {
                let s = in_slice[i * stride];
                Pos2::new(rect.left() + i as f32 * step_x, mid_y - s.clamp(-1.0, 1.0) * amp)
            })
            .collect();

        let bass_points: Vec<Pos2> = (0..draw_count)
            .map(|i| {
                let s = bass_slice[i * stride];
                Pos2::new(rect.left() + i as f32 * step_x, mid_y - s.clamp(-1.0, 1.0) * amp)
            })
            .collect();

        if in_points.len() >= 2 {
            painter.add(Shape::line(
                in_points,
                Stroke::new(1.1, Color32::from_rgba_unmultiplied(104, 138, 178, 150)),
            ));
        }
        if bass_points.len() >= 2 {
            painter.add(Shape::line(bass_points, Stroke::new(2.0, ACCENT_AMBER)));
        }
    } else {
        painter.text(
            Pos2::new(rect.right() - 8.0, rect.top() + 8.0),
            egui::Align2::RIGHT_TOP,
            "Idle",
            egui::FontId::monospace(10.0),
            TEXT_DIM,
        );
    }

    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 8.0),
        egui::Align2::LEFT_TOP,
        "Input",
        egui::FontId::proportional(10.5),
        ACCENT_BLUE,
    );
    painter.text(
        Pos2::new(rect.left() + 50.0, rect.top() + 8.0),
        egui::Align2::LEFT_TOP,
        "Bass",
        egui::FontId::proportional(10.5),
        ACCENT_AMBER,
    );
}

// ==========================================
// HORIZONTAL LEVEL METER WITH dBFS READOUT
// ==========================================
fn render_level_meter(ui: &mut egui::Ui, label: &str, peak: f32, bar_color: Color32) {
    let db_str = if peak > 0.0005 {
        format!("{:.0} dB", (20.0 * peak.log10()).clamp(-60.0, 6.0))
    } else {
        "-inf".to_string()
    };

    let db = if peak > 0.0001 {
        20.0 * peak.log10()
    } else {
        -60.0
    };

    let bar_w = 54.0;
    let bar_h = 7.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_w + 68.0, 16.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.text(
        Pos2::new(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::monospace(10.0),
        TEXT_MUTED,
    );

    let bar_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 26.0, rect.center().y - bar_h * 0.5),
        Vec2::new(bar_w, bar_h),
    );
    painter.rect_filled(bar_rect, 2.0, BG_PLOT);
    painter.rect_stroke(bar_rect, 2.0, Stroke::new(0.8, BORDER));

    let frac = ((db + 48.0) / 48.0).clamp(0.0, 1.0);
    if frac > 0.01 {
        let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(bar_w * frac, bar_h));
        let fill_color = if db > -0.5 { STATUS_RED } else { bar_color };
        painter.rect_filled(fill_rect, 2.0, fill_color);
    }

    painter.text(
        Pos2::new(rect.right(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        db_str,
        egui::FontId::monospace(9.5),
        TEXT_DIM,
    );
}
