#![allow(non_snake_case)]
use analog_console::{AnalogConsoleProcessor, SaturationType};
use auto_compressor::SimpleAutoCompressor;
use db_meter::DBMeter;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, FontId, Rect, RichText, CornerRadius},
    widgets, EguiState,
};
mod BoolButton;
use std::sync::Arc;
mod db_meter;
mod analog_console;
mod auto_compressor;

/**************************************************
 * UnderBrush v1.0.1 by Ardura
 *
 * Build with: cargo xtask bundle underbrush --profile release
 * Debug with: cargo xtask bundle underbrush --profile profiling
 *
 * ************************************************/

const DARK_GREEN: Color32 = Color32::from_rgb(40, 54, 24);
const LIGHT_GREEN: Color32 = Color32::from_rgb(96, 108, 56);
const ORANGE: Color32 = Color32::from_rgb(188, 108, 37);

/// The time it takes for the peak meter to decay by 12 dB after switching to complete silence.
const PEAK_METER_DECAY_MS: f64 = 100.0;

pub struct UnderBrush {
    params: Arc<UnderBrushParams>,
    // The current data for the different meters
    out_meter: Arc<AtomicF32>,
    in_meter: Arc<AtomicF32>,
    // normalize the peak meter's response based on the sample rate with this
    out_meter_decay_weight: f32,

    // Slew History
    prev_slew_l: f32,
    prev_slew_r: f32,

    // Console
    console: analog_console::AnalogConsoleProcessor,

    // Compression
    compressor: auto_compressor::SimpleAutoCompressor,
}

#[derive(Params)]
struct UnderBrushParams {
    /// The editor state
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    /// Slew Limiting
    #[id = "slew"]
    pub slew: FloatParam,

    /// Console Drive
    #[id = "drive"]
    pub drive: FloatParam,

    /// Console Saturation Type
    #[id = "type"]
    pub sat_type: EnumParam<SaturationType>,

    /// Linearizer Frequency
    #[id = "Linearizer Hz"]
    pub l_hz: FloatParam,

    /// Compressor
    #[id = "Comp"]
    pub comp: BoolParam,

    /// Clipper
    #[id = "Clip at 0db"]
    pub clip: BoolParam,

    /// Console Wet/Dry
    #[id = "mix"]
    pub mix: FloatParam,

    /// Console Signal Gain
    #[id = "gain"]
    pub gain: FloatParam,

    /// Master out
    #[id = "Master Out"]
    pub master_out: FloatParam,
}

impl Default for UnderBrush {
    fn default() -> Self {
        Self {
            params: Arc::new(UnderBrushParams::default()),
            out_meter_decay_weight: 1.0,
            out_meter: Arc::new(AtomicF32::new(util::MINUS_INFINITY_DB)),
            in_meter: Arc::new(AtomicF32::new(util::MINUS_INFINITY_DB)),
            prev_slew_l: 0.0,
            prev_slew_r: 0.0,
            console: AnalogConsoleProcessor::new(44100.0),
            compressor: SimpleAutoCompressor::new(44100.0),
        }
    }
}

impl Default for UnderBrushParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(250, 290),
            slew: FloatParam::new(
                "Slew",
                0.8,
                FloatRange::Skewed { min: 0.00001, max: 1.0, factor: 0.3 },
            )
            .with_step_size(0.00001),
            drive: FloatParam::new(
                "Drive",
                1.0,
                FloatRange::Skewed { min: 0.00001, max: 10.0, factor: 0.3 },
            )
            .with_step_size(0.00001),
            sat_type: EnumParam::new("Type", SaturationType::Tape),
            l_hz: FloatParam::new(
                "Lin Hz",
                150.0,
                FloatRange::Linear { min: 20.0, max: 800.0 },
            )
            .with_step_size(1.0),
            comp: BoolParam::new("Compression", false),
            clip: BoolParam::new("Clip at 0db", false),
            mix: FloatParam::new(
                "Mix",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_step_size(0.00001),
            gain: FloatParam::new(
                "Gain",
                0.0,
                FloatRange::Linear { min: -12.0, max: 12.0 },
            )
            .with_step_size(0.00001),
            master_out: FloatParam::new(
                "Master",
                0.0,
                FloatRange::Linear { min: -24.0, max: 24.0 },
            )
            .with_step_size(0.00001),
        }
    }
}

impl Plugin for UnderBrush {
    const NAME: &'static str = "Underbrush";
    const VENDOR: &'static str = "Ardura";
    const URL: &'static str = "https://github.com/ardura";
    const EMAIL: &'static str = "azviscarra@gmail.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // This looks like it's flexible for running the plugin in mono or stereo
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let in_meter = self.in_meter.clone();
        let out_meter = self.out_meter.clone();
        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    // Change colors - there's probably a better way to do this
                    let style_var = ui.style_mut();
                    style_var.visuals.widgets.inactive.bg_fill = Color32::GRAY;

                    // Assign default colors if user colors not set
                    style_var.visuals.widgets.inactive.fg_stroke.color = LIGHT_GREEN;
                    style_var.visuals.widgets.noninteractive.fg_stroke.color = ORANGE;
                    style_var.visuals.widgets.inactive.bg_stroke.color = ORANGE;
                    style_var.visuals.widgets.active.fg_stroke.color = LIGHT_GREEN;
                    style_var.visuals.widgets.active.bg_stroke.color = ORANGE;
                    style_var.visuals.widgets.open.fg_stroke.color = ORANGE;
                    // Param fill
                    style_var.visuals.selection.bg_fill = ORANGE;

                    style_var.visuals.widgets.noninteractive.bg_stroke.color = Color32::GRAY;
                    style_var.visuals.widgets.noninteractive.bg_fill = Color32::GRAY;

                    // Trying to draw background as rect
                    ui.painter()
                        .rect_filled(Rect::EVERYTHING, CornerRadius::ZERO, DARK_GREEN);

                    // The entire "window" container
                    ui.vertical(|ui| {
                        ui.label("UnderBrush")
                            .on_hover_text("by Ardura with nih-plug and egui");

                        // Peak Meters
                        let in_meter =
                            util::gain_to_db(in_meter.load(std::sync::atomic::Ordering::Relaxed));
                        let in_meter_text = if in_meter > util::MINUS_INFINITY_DB {
                            format!("{in_meter:.1} dBFS Input")
                        } else {
                            String::from("-inf dBFS Input")
                        };
                        let in_meter_normalized = (in_meter + 60.0) / 60.0;
                        ui.allocate_space(egui::Vec2::splat(2.0));
                        let in_meter_obj = DBMeter::new(in_meter_normalized).text(in_meter_text);
                        ui.add(in_meter_obj);

                        let out_meter =
                            util::gain_to_db(out_meter.load(std::sync::atomic::Ordering::Relaxed));
                        let out_meter_text = if out_meter > util::MINUS_INFINITY_DB {
                            format!("{out_meter:.1} dBFS Output")
                        } else {
                            String::from("-inf dBFS Output")
                        };
                        let out_meter_normalized = (out_meter + 60.0) / 60.0;
                        ui.allocate_space(egui::Vec2::splat(2.0));
                        let out_meter_obj = DBMeter::new(out_meter_normalized).text(out_meter_text);
                        ui.add(out_meter_obj);

                        // Sliders
                        let monofont = FontId::monospace(12.0);

                        ui.horizontal(|ui|{
                            ui.label(RichText::new("Drive").font(monofont.clone()));
                            ui.add(
                                widgets::ParamSlider::for_param(&params.drive, setter)
                                    .with_width(130.0),
                            )
                            .on_hover_text("Signal overdrive to console");
                        });

                        ui.horizontal(|ui|{
                            ui.label(RichText::new("Type ").font(monofont.clone()));
                            ui.add(
                                widgets::ParamSlider::for_param(&params.sat_type, setter)
                                    .with_width(130.0),
                            )
                            .on_hover_text("The style of saturation");
                        });

                        ui.horizontal(|ui|{
                            ui.label(RichText::new("Lin Hz").font(monofont.clone()));
                            ui.add(
                                widgets::ParamSlider::for_param(&params.l_hz, setter)
                                    .with_width(120.0),
                            )
                            .on_hover_text("Frequency Cutoff for the linearizer.
A phase linearizer aligns
sound frequencies in time");
                        });

                        // Fix bypass switch being LOUD
                        if *&params.sat_type.value() == SaturationType::Bypass && *&params.drive.value() != 1.0 {
                            setter.begin_set_parameter(&params.drive);
                            setter.set_parameter(&params.drive, 1.0);
                            setter.end_set_parameter(&params.drive);
                        }

                        ui.horizontal(|ui|{
                            ui.label(RichText::new("Slew ").font(monofont.clone()));
                            ui.add(
                                widgets::ParamSlider::for_param(&params.slew, setter)
                                    .with_width(130.0),
                            )
                            .on_hover_text("What rate of change is allowed (limiting)");
                        });

                        ui.vertical_centered(|ui|{
                            ui.add(
                                BoolButton::BoolButton::for_param(&params.comp, setter, 5.0, 1.0, monofont.clone()),
                            )
                            .on_hover_text("Gentle auto compression");
                        });

                        ui.horizontal(|ui|{
                            ui.label(RichText::new("Gain ").font(monofont.clone()));
                            ui.add(
                                widgets::ParamSlider::for_param(&params.gain, setter)
                                    .with_width(130.0),
                            )
                            .on_hover_text("Output gain of signal");
                        });

                        ui.vertical_centered(|ui|{
                            ui.add(
                                BoolButton::BoolButton::for_param(&params.clip, setter, 5.0, 1.0, monofont.clone()),
                            )
                            .on_hover_text("Keep signal below 0db forcefully");
                        });

                        ui.horizontal(|ui|{
                            ui.label(RichText::new("Mix  ").font(monofont.clone()));
                            ui.add(
                                widgets::ParamSlider::for_param(&params.mix, setter)
                                    .with_width(130.0),
                            )
                            .on_hover_text("Wet/Dry of the processing effect");
                        });

                        ui.horizontal(|ui|{
                            ui.label(RichText::new("Master").font(monofont.clone()));
                            ui.add(
                                widgets::ParamSlider::for_param(&params.master_out, setter)
                                    .with_width(100.0),
                            )
                            .on_hover_text("Master volume of output");
                        });
                    });
                });
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // After `PEAK_METER_DECAY_MS` milliseconds of pure silence, the peak meter's value should
        // have dropped by 12 dB
        self.out_meter_decay_weight = 0.25f64
            .powf((buffer_config.sample_rate as f64 * PEAK_METER_DECAY_MS / 1000.0).recip())
            as f32;

        true
    }

    fn process(
        &mut self,
        buffer: &mut nih_plug::prelude::Buffer<'_>,
        _aux: &mut nih_plug::prelude::AuxiliaryBuffers<'_>,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let slew: f32 = self.params.slew.value();
        let current_sample_rate = _context.transport().sample_rate;
        let overallscale = current_sample_rate / 44100.0;
        
        self.console.set_sample_rate(current_sample_rate);
        self.console.set_drive(self.params.drive.value());
        self.console.set_saturation_type(self.params.sat_type.value());
        self.console.set_crosstalk(0.03);
        self.console.set_phase_linearizer_freq(self.params.l_hz.value());

        self.compressor.set_sample_rate(current_sample_rate);

        let mix = self.params.mix.value();

        for mut channel_samples in buffer.iter_samples() {
            // Get the length of our buffer to use later
            let num_samples = channel_samples.len();
            let localthreshold = slew / overallscale;

            // Split left and right same way original subhoofer did
            let mut out_l = *channel_samples.get_mut(0).unwrap();
            let mut out_r = *channel_samples.get_mut(1).unwrap();
            let dry_left = out_l;
            let dry_right = out_r;

            let mut in_amplitude: f32 = (out_l + out_r / 2.0).abs();

            // Main Processing
            (out_l, out_r) = self.console.process(out_l, out_r);

            // Slew limiting
            let mut clamp = out_l - self.prev_slew_l;
            if clamp > localthreshold {
                out_l = self.prev_slew_l + localthreshold;
            }
            if -clamp > localthreshold {
                out_l = self.prev_slew_l - localthreshold;
            }
            self.prev_slew_l = out_l;

            clamp = out_r - self.prev_slew_r;
            if clamp > localthreshold {
                out_r = self.prev_slew_r + localthreshold;
            }
            if -clamp > localthreshold {
                out_r = self.prev_slew_r - localthreshold;
            }
            self.prev_slew_r = out_r;

            if self.params.comp.value() {
                out_l = self.compressor.process(out_l);
                out_r = self.compressor.process(out_r);
            }

            out_l = out_l * util::db_to_gain(self.params.gain.value());
            out_r = out_r * util::db_to_gain(self.params.gain.value());

            // Safety for our ears
            if self.params.clip.value() {
                out_l = out_l.clamp(-0.9999, 0.9999);
                out_r = out_r.clamp(-0.9999, 0.9999);
            }

            // Mix dry/wet
            out_l = (1.0 - mix) * dry_left + mix * out_l;
            out_r = (1.0 - mix) * dry_right + mix * out_r;

            // Assign our output
            *channel_samples.get_mut(0).unwrap() = out_l;
            *channel_samples.get_mut(1).unwrap() = out_r;

            ///////////////////////////////////////////////////////////////////////////////

            let mut out_amplitude = out_l + out_r;

            // Only process the meters if the GUI is open
            if self.params.editor_state.is_open() {
                // Input gain meter
                in_amplitude = (in_amplitude / num_samples as f32).abs();
                let current_in_meter: f32 =
                    self.in_meter.load(std::sync::atomic::Ordering::Relaxed);
                let new_in_meter = if in_amplitude > current_in_meter {
                    in_amplitude
                } else {
                    current_in_meter * self.out_meter_decay_weight
                        + in_amplitude * (1.0 - self.out_meter_decay_weight)
                };
                self.in_meter
                    .store(new_in_meter, std::sync::atomic::Ordering::Relaxed);

                // Output gain meter
                out_amplitude = (out_amplitude / num_samples as f32).abs();
                let current_out_meter = self.out_meter.load(std::sync::atomic::Ordering::Relaxed);
                let new_out_meter = if out_amplitude > current_out_meter {
                    out_amplitude
                } else {
                    current_out_meter * self.out_meter_decay_weight
                        + out_amplitude * (1.0 - self.out_meter_decay_weight)
                };
                self.out_meter
                    .store(new_out_meter, std::sync::atomic::Ordering::Relaxed);
            }
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for UnderBrush {
    const CLAP_ID: &'static str = "com.ardura.underbrush";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Analog Console");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Compressor,
    ];
}

impl Vst3Plugin for UnderBrush {
    const VST3_CLASS_ID: [u8; 16] = *b"underbrushAAAAAA";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(UnderBrush);
nih_export_vst3!(UnderBrush);
