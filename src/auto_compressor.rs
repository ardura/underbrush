
/// A simplified automatic compressor with dynamic ratio system
pub struct SimpleAutoCompressor {
    sample_rate: f32,
    envelope: f32,
    gain_reduction: f32,
    
    attack_coeff: f32,
    release_coeff: f32,
    
    // Level tracking
    peak_average: f32,
    
    // Meters
    pub input_level: f32,
    pub output_level: f32,
    pub gain_reduction_db: f32,
}

impl SimpleAutoCompressor {
    pub fn new(sample_rate: f32) -> Self {
        let attack_ms = 15.0;
        let release_ms = 200.0;
        
        Self {
            sample_rate,
            envelope: 0.0,
            gain_reduction: 1.0,
            
            // Pre-calculate coefficients
            attack_coeff: (-1.0 / (attack_ms * 0.001 * sample_rate)).exp(),
            release_coeff: (-1.0 / (release_ms * 0.001 * sample_rate)).exp(),
            
            peak_average: 0.0,
            
            input_level: 0.0,
            output_level: 0.0,
            gain_reduction_db: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.attack_coeff = (-1.0 / (15.0 * 0.001 * sample_rate)).exp();
        self.release_coeff = (-1.0 / (200.0 * 0.001 * sample_rate)).exp();
    }
    
    /// Process a single sample of audio
    pub fn process(&mut self, input: f32) -> f32 {
        // Update input level
        self.input_level = 0.9 * self.input_level + 0.1 * input.abs();
        
        let input_abs = input.abs();
        
        if input_abs > self.envelope {
            self.envelope = input_abs * (1.0 - self.attack_coeff) + self.envelope * self.attack_coeff;
        } else {
            self.envelope = input_abs * (1.0 - self.release_coeff) + self.envelope * self.release_coeff;
        }
        
        // Update peak memory with simple averaging
        self.peak_average = 0.995 * self.peak_average + 0.005 * self.envelope;
        
        // Simple auto-threshold based on recent peak average
        let threshold = self.peak_average * 0.5;
        
        // Calculate gain reduction with dynamic ratio
        if self.envelope <= threshold {
            self.gain_reduction = 1.0;
        } else {
            // Calculate how far above threshold we are (in dB)
            let excess_db = 20.0 * (self.envelope / threshold).log10();
            let ratio = self.calculate_dynamic_ratio(excess_db);
            
            let reduction_db = excess_db - (excess_db / ratio);
            let target_gain = 10.0_f32.powf(-reduction_db / 20.0);
            
            self.gain_reduction = 0.9 * self.gain_reduction + 0.1 * target_gain;
        }
        
        // Apply compression
        let output = input * self.gain_reduction;
        
        // Simple makeup gain
        let makeup_gain = 1.4;
        let output_with_makeup = output * makeup_gain;
        
        // Update meters
        self.gain_reduction_db = -20.0 * self.gain_reduction.log10();
        self.output_level = 0.9 * self.output_level + 0.1 * output_with_makeup.abs();
        
        output_with_makeup
    }
    
    /// Calculate dynamic ratio based on how far above threshold the signal is
    fn calculate_dynamic_ratio(&self, excess_db: f32) -> f32 {
        // Progressive ratio:
        // - Gentle (1.5:1) for just above threshold
        // - Firmer (4:1) for significantly above threshold
        let min_ratio = 1.5;
        let max_ratio = 4.0;
        
        // Clamp the excess to a reasonable range for ratio calculation
        let clamped_excess = excess_db.min(20.0);
        
        // Linear interpolation based on excess level
        min_ratio + (clamped_excess / 20.0) * (max_ratio - min_ratio)
    }
}