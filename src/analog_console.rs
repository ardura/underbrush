use std::f32::consts::PI;
use std::collections::VecDeque;

use nih_plug::prelude::Enum;

/// Analog-style console processor with saturation, EQ, crosstalk, and phase linearization
pub struct AnalogConsoleProcessor {
    // Saturation parameters
    drive: f32,
    saturation_type: SaturationType,

    // Crosstalk parameters
    crosstalk_amount: f32,

    // Internal state
    _prev_left: f32,
    _prev_right: f32,

    // DC blocker
    _dc_blocker_left: DCBlocker,
    _dc_blocker_right: DCBlocker,

    // Phase linearizer
    phase_linearizer_left: DCPhaseLinearizer,
    phase_linearizer_right: DCPhaseLinearizer,
}

#[derive(Clone, Copy, PartialEq, Enum)]
pub enum SaturationType {
    Tape,
    Tube,
    Transistor,
    LDR,
    Cubic,
    Quintic,
    SoftClip,
    Bypass,
}

impl AnalogConsoleProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            drive: 0.5,
            saturation_type: SaturationType::Tape,
            crosstalk_amount: 0.05,
            _prev_left: 0.0,
            _prev_right: 0.0,
            _dc_blocker_left: DCBlocker::new(0.995),
            _dc_blocker_right: DCBlocker::new(0.995),
            phase_linearizer_left: DCPhaseLinearizer::new(sample_rate, 30.0),
            phase_linearizer_right: DCPhaseLinearizer::new(sample_rate, 30.0),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.phase_linearizer_left.set_sample_rate(sample_rate);
        self.phase_linearizer_right.set_sample_rate(sample_rate);
    }

    pub fn set_drive(&mut self, drive: f32) {
        self.drive = drive.clamp(1.0, 10.0);
    }

    pub fn set_saturation_type(&mut self, sat_type: SaturationType) {
        self.saturation_type = sat_type;
    }

    pub fn set_crosstalk(&mut self, amount: f32) {
        self.crosstalk_amount = amount.clamp(0.0, 0.3);
    }

    pub fn set_phase_linearizer_freq(&mut self, freq_hz: f32) {
        if self.phase_linearizer_left.corner_freq != freq_hz {
            self.phase_linearizer_left.set_corner_frequency(freq_hz);
        }
        if self.phase_linearizer_right.corner_freq != freq_hz {
            self.phase_linearizer_right.set_corner_frequency(freq_hz);
        }
    }

    /// Process a single stereo sample
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // Apply soft saturation
        let left_sat = self.saturate(left);
        let right_sat = self.saturate(right);

        // Apply crosstalk
        let left_cross = (1.0 - self.crosstalk_amount) * left_sat + self.crosstalk_amount * right_sat;
        let right_cross = (1.0 - self.crosstalk_amount) * right_sat + self.crosstalk_amount * left_sat;

        // Subtle envelope following (transient smoothing)
        let left_smooth = 0.9 * left_cross + 0.1 * self._prev_left;
        let right_smooth = 0.9 * right_cross + 0.1 * self._prev_right;

        self._prev_left = left_cross;
        self._prev_right = right_cross;

        // Apply DC blocking to avoid unwanted offsets from the saturation
        let left_dc_blocked = self._dc_blocker_left.process(left_smooth);
        let right_dc_blocked = self._dc_blocker_right.process(right_smooth);

        // Apply phase linearization
        let left_linearized = self.phase_linearizer_left.process(left_dc_blocked);
        let right_linearized = self.phase_linearizer_right.process(right_dc_blocked);

        (left_linearized, right_linearized)
    }

    fn saturate(&self, sample: f32) -> f32 {
        let driven = sample * self.drive;

        match self.saturation_type {
            SaturationType::Tape => {
                let factor = self.drive + 1.0;
                (sample * factor).tanh() * 0.5
            },
            SaturationType::Tube => {
                // Tube-style asymmetric saturation (warmer on positive, sharper on negative)
                if driven >= 0.0 {
                    1.0 - ((-driven).exp())
                } else {
                    -1.0 + ((driven).exp())
                }
            },
            SaturationType::Transistor => {
                // Transistor-style harder clipping with some curve
                driven / (1.0 + driven.abs().powf(1.5)) * 1.2
            },
            SaturationType::LDR => {
                // Simplified model of LDR behavior: resistance decreases with increasing input level.
                let control_signal = driven.abs().clamp(0.0, 1.0);
                let resistance = 1.0 / (0.1 + control_signal * 5.0);
                let saturation_scaler = 0.83;

                driven / (1.0 + resistance * saturation_scaler)
            },
            SaturationType::Cubic => {
                sample + self.drive * sample * sample * sample
            },
            SaturationType::Quintic => {
                let drive1 = 0.5 * self.drive;
                let drive2 = 0.3 * self.drive;
                sample + drive1 * sample.powi(3) + drive2 * sample.powi(5)
            },
            SaturationType::SoftClip => {
                driven / (1.0 + driven.abs())
            },
            SaturationType::Bypass => {
                driven
            },
        }
    }
}

/// DC Phase Linearizer - Preserves phase relationship in low frequencies
pub struct DCPhaseLinearizer {
    sample_rate: f32,
    corner_freq: f32,
    buffer: VecDeque<f32>,
    allpass_filter: AllpassFilter,
    delay_samples: usize,
}

impl DCPhaseLinearizer {
    pub fn new(sample_rate: f32, corner_freq_hz: f32) -> Self {
        let allpass = AllpassFilter::new(sample_rate, corner_freq_hz);
        let delay_samples = (sample_rate / corner_freq_hz * 0.25) as usize;

        let mut buffer = VecDeque::with_capacity(delay_samples + 1);
        buffer.resize(delay_samples, 0.0);

        Self {
            sample_rate,
            corner_freq: corner_freq_hz,
            buffer,
            allpass_filter: allpass,
            delay_samples,
        }
    }

    pub fn set_sample_rate(&mut self, new_sample_rate: f32) {
        self.sample_rate = new_sample_rate;
        self.allpass_filter.set_sample_rate(new_sample_rate);
        // Recalculate delay based on the new sample rate and current corner frequency
        //let new_delay = (self.sample_rate / self.corner_freq * 0.25) as usize;
        let new_delay = (self.sample_rate / (PI * self.corner_freq) - 1.0).max(0.0) as usize;
        if new_delay != self.delay_samples {
            self.delay_samples = new_delay;
            self.buffer.resize(self.delay_samples, 0.0);
        }
    }

    pub fn set_corner_frequency(&mut self, freq_hz: f32) {
        self.corner_freq = freq_hz.clamp(20.0, 800.0);
        self.allpass_filter.set_frequency(self.corner_freq);

        // Recalculate delay when frequency changes
        //let new_delay = (self.sample_rate / self.corner_freq * 0.25) as usize;
        let new_delay = (self.sample_rate / (PI * self.corner_freq) - 1.0).max(0.0) as usize;

        // Resize buffer if needed
        if new_delay != self.delay_samples {
            self.delay_samples = new_delay;
            self.buffer.resize(self.delay_samples, 0.0);
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        // Process through allpass
        let allpass_out = self.allpass_filter.process(input);

        // Store in delay buffer
        self.buffer.push_back(allpass_out);

        // Get delayed sample
        let delayed = if let Some(sample) = self.buffer.pop_front() {
            sample
        } else {
            0.0
        };

        let crossover_coeff = 0.3;
        let low_mix_alt = input * crossover_coeff + delayed * (1.0 - crossover_coeff);
        let output = low_mix_alt * 0.7 + allpass_out * 0.3;
        
        output
    }
}

/// Allpass filter for phase manipulation
pub struct AllpassFilter {
    a1: f32,
    z1: f32,
    sample_rate: f32,
}

impl AllpassFilter {
    pub fn new(sample_rate: f32, freq_hz: f32) -> Self {
        let a1 = Self::calculate_coefficient(freq_hz, sample_rate);

        Self {
            a1,
            z1: 0.0,
            sample_rate,
        }
    }

    fn calculate_coefficient(freq_hz: f32, sample_rate: f32) -> f32 {
        let t = (PI * freq_hz / sample_rate).tan();
        (t - 1.0) / (t + 1.0)
    }

    pub fn set_sample_rate(&mut self, new_sample_rate: f32) {
        self.sample_rate = new_sample_rate;
        // Recalculate the coefficient based on the new sample rate and current frequency
        self.a1 = Self::calculate_coefficient(self.get_frequency(), self.sample_rate);
    }

    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.a1 = Self::calculate_coefficient(freq_hz, self.sample_rate);
    }

    pub fn get_frequency(&self) -> f32 {
        // This is a reverse calculation of the coefficient to get the frequency.
        // It might not be perfectly accurate due to floating-point precision,
        // but it should be good enough for recalculating the coefficient.
        let t_minus_one = self.a1;
        let t_plus_one = 1.0;
        let t = (t_minus_one + t_plus_one) / (t_plus_one - t_minus_one);
        (self.sample_rate * t.atan()) / PI
    }

    pub fn process(&mut self, input: f32) -> f32 {
        // First-order allpass formula: y[n] = a1*x[n] + x[n-1] - a1*y[n-1]
        let output = self.a1 * input + self.z1;
        self.z1 = input - self.a1 * output;
        output
    }
}

/// DC blocker to remove DC offset introduced by asymmetric saturation
pub struct DCBlocker {
    r: f32,
    x1: f32,
    y1: f32,
}

impl DCBlocker {
    pub fn new(r: f32) -> Self {
        Self {
            r: r.clamp(0.9, 0.999),
            x1: 0.0,
            y1: 0.0,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let output = input - self.x1 + self.r * self.y1;
        self.x1 = input;
        self.y1 = output;
        output
    }
}