//! Audio bandpass filter — instelbaar high-pass en low-pass via biquad IIR.
//!
//! Twee onafhankelijk in/uit-schakelbare cascaded biquad filters:
//! - HP (high-pass): rolt sub-bas weg (bv. 50 Hz brom)
//! - LP (low-pass): rolt highs weg (bv. boven 6 kHz)
//!
//! Default: beide uit (`set_hp_hz(0)` / `set_lp_hz(0)` betekent bypass).
//! Stereo wordt als interleaved L,R,L,R verwerkt; elke kanaal heeft zijn
//! eigen state.

use std::f32::consts::PI;

/// Vaste filter presets, kiesbaar via CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFilterPreset {
    /// Geen filter (bypass).
    Off,
    /// Breed: 100-6000 Hz. Goed voor muziek-luisteren of data-modes.
    Wide,
    /// Spraak: 100-3000 Hz. Algemene voice.
    Voice,
    /// SSB standaard: 150-2800 Hz. Klassieke amateur-SSB bandbreedte.
    Ssb,
    /// Smal: 200-2800 Hz. Voor zwakke signalen / minder LF brom.
    Narrow,
}

impl AudioFilterPreset {
    /// Geeft (hp_hz, lp_hz). 0 = uit.
    pub fn cutoffs(&self) -> (f32, f32) {
        match self {
            Self::Off => (0.0, 0.0),
            Self::Wide => (100.0, 6000.0),
            Self::Voice => (100.0, 3000.0),
            Self::Ssb => (150.0, 2800.0),
            Self::Narrow => (200.0, 2800.0),
        }
    }
}

/// Een tweede-orde IIR biquad section (Direct Form II Transposed).
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    // Coefficients (genormaliseerd op a0 = 1)
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    // State
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn passthrough() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Reset alleen de state, niet de coefficients. Gebruik dit bij
    /// stream-discontinuïteiten of bij parameter-wijziging om transients
    /// te vermijden (kleine "plop" bij grote coefficient-sprong).
    fn reset_state(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// RBJ butterworth-Q biquad cookbook: 2-poles high-pass.
    /// fc = cutoff Hz, fs = sample rate Hz.
    fn set_highpass(&mut self, fc: f32, fs: f32) {
        let q = 0.7071; // Butterworth Q
        let w0 = 2.0 * PI * fc / fs;
        let cos_w = w0.cos();
        let sin_w = w0.sin();
        let alpha = sin_w / (2.0 * q);
        let a0 = 1.0 + alpha;
        let b0 = (1.0 + cos_w) / 2.0;
        let b1 = -(1.0 + cos_w);
        let b2 = (1.0 + cos_w) / 2.0;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// RBJ butterworth-Q biquad cookbook: 2-poles low-pass.
    fn set_lowpass(&mut self, fc: f32, fs: f32) {
        let q = 0.7071;
        let w0 = 2.0 * PI * fc / fs;
        let cos_w = w0.cos();
        let sin_w = w0.sin();
        let alpha = sin_w / (2.0 * q);
        let a0 = 1.0 + alpha;
        let b0 = (1.0 - cos_w) / 2.0;
        let b1 = 1.0 - cos_w;
        let b2 = (1.0 - cos_w) / 2.0;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        // Direct Form II Transposed
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Bandpass filter (HP + LP cascade). Stereo, beide kanalen apart.
#[derive(Debug, Clone)]
pub struct BandpassFilter {
    sample_rate: f32,
    hp_hz: f32,
    lp_hz: f32,
    // 2 cascaded biquads per kanaal: HP + LP
    hp_l: Biquad,
    lp_l: Biquad,
    hp_r: Biquad,
    lp_r: Biquad,
    hp_active: bool,
    lp_active: bool,
}

impl BandpassFilter {
    /// Maak een nieuwe filter. `hp_hz` of `lp_hz` op 0 betekent uit.
    pub fn new(sample_rate: u32, hp_hz: f32, lp_hz: f32) -> Self {
        let mut f = Self {
            sample_rate: sample_rate as f32,
            hp_hz: 0.0,
            lp_hz: 0.0,
            hp_l: Biquad::passthrough(),
            lp_l: Biquad::passthrough(),
            hp_r: Biquad::passthrough(),
            lp_r: Biquad::passthrough(),
            hp_active: false,
            lp_active: false,
        };
        f.set_hp_hz(hp_hz);
        f.set_lp_hz(lp_hz);
        f
    }

    /// Maak een filter met cutoffs uit een preset.
    pub fn from_preset(sample_rate: u32, preset: AudioFilterPreset) -> Self {
        let (hp, lp) = preset.cutoffs();
        Self::new(sample_rate, hp, lp)
    }

    /// Zet de high-pass cutoff. 0 = uit.
    /// Veilig range: 1 Hz tot fs/4 (boven nyquist/2 levert onstabiele biquad).
    pub fn set_hp_hz(&mut self, hz: f32) {
        let new_active = hz > 0.5;
        let new_hz = if new_active {
            hz.clamp(1.0, self.sample_rate / 4.0)
        } else {
            0.0
        };
        if new_active != self.hp_active || new_hz != self.hp_hz {
            self.hp_hz = new_hz;
            self.hp_active = new_active;
            if new_active {
                self.hp_l.set_highpass(new_hz, self.sample_rate);
                self.hp_r.set_highpass(new_hz, self.sample_rate);
            } else {
                self.hp_l = Biquad::passthrough();
                self.hp_r = Biquad::passthrough();
            }
            // Reset state om plop te vermijden
            self.hp_l.reset_state();
            self.hp_r.reset_state();
        }
    }

    /// Zet de low-pass cutoff. 0 = uit.
    pub fn set_lp_hz(&mut self, hz: f32) {
        let new_active = hz > 0.5;
        let new_hz = if new_active {
            hz.clamp(1.0, self.sample_rate / 4.0)
        } else {
            0.0
        };
        if new_active != self.lp_active || new_hz != self.lp_hz {
            self.lp_hz = new_hz;
            self.lp_active = new_active;
            if new_active {
                self.lp_l.set_lowpass(new_hz, self.sample_rate);
                self.lp_r.set_lowpass(new_hz, self.sample_rate);
            } else {
                self.lp_l = Biquad::passthrough();
                self.lp_r = Biquad::passthrough();
            }
            self.lp_l.reset_state();
            self.lp_r.reset_state();
        }
    }

    /// Pas de sample rate aan (her-bereken coefficients zonder state reset).
    pub fn set_sample_rate(&mut self, fs: u32) {
        let new_fs = fs as f32;
        if (new_fs - self.sample_rate).abs() < 0.5 {
            return;
        }
        self.sample_rate = new_fs;
        if self.hp_active {
            self.hp_l.set_highpass(self.hp_hz, new_fs);
            self.hp_r.set_highpass(self.hp_hz, new_fs);
        }
        if self.lp_active {
            self.lp_l.set_lowpass(self.lp_hz, new_fs);
            self.lp_r.set_lowpass(self.lp_hz, new_fs);
        }
    }

    /// Geeft true als minimaal één van de filters actief is.
    pub fn is_active(&self) -> bool {
        self.hp_active || self.lp_active
    }

    pub fn hp_hz(&self) -> f32 {
        self.hp_hz
    }

    pub fn lp_hz(&self) -> f32 {
        self.lp_hz
    }

    /// In-place stereo verwerking. `samples` is interleaved L,R,L,R...
    /// Als geen filter actief is, doet deze functie niets (bypass).
    pub fn process_stereo(&mut self, samples: &mut [f32]) {
        if !self.is_active() {
            return;
        }
        let mut i = 0;
        let n = samples.len();
        while i + 1 < n {
            let mut l = samples[i];
            let mut r = samples[i + 1];
            if self.hp_active {
                l = self.hp_l.process(l);
                r = self.hp_r.process(r);
            }
            if self.lp_active {
                l = self.lp_l.process(l);
                r = self.lp_r.process(r);
            }
            samples[i] = l;
            samples[i + 1] = r;
            i += 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypass_when_inactive() {
        let mut f = BandpassFilter::new(48000, 0.0, 0.0);
        let mut s = vec![0.5, -0.3, 0.1, 0.7];
        let orig = s.clone();
        f.process_stereo(&mut s);
        assert_eq!(s, orig);
    }

    #[test]
    fn hp_removes_dc() {
        let mut f = BandpassFilter::new(48000, 100.0, 0.0);
        // DC signaal (constante waarde) moet weg na een paar samples
        let mut s = vec![1.0_f32; 48000];
        // Stereo-ify
        let mut stereo: Vec<f32> = s.iter().flat_map(|&v| [v, v]).collect();
        f.process_stereo(&mut stereo);
        // Laatste 100 samples moeten dicht bij nul liggen
        let tail: Vec<f32> = stereo[stereo.len() - 200..].to_vec();
        let max_abs = tail.iter().map(|x| x.abs()).fold(0.0_f32, f32::max);
        assert!(max_abs < 0.01, "DC niet onderdrukt: max_abs = {}", max_abs);
    }

    #[test]
    fn lp_removes_high_freq() {
        let mut f = BandpassFilter::new(48000, 0.0, 1000.0);
        // 8 kHz sinus, ver boven cutoff
        let fs = 48000.0;
        let freq = 8000.0;
        let mut stereo: Vec<f32> = (0..4800)
            .flat_map(|i| {
                let v = (2.0 * PI * freq * (i as f32) / fs).sin();
                [v, v]
            })
            .collect();
        f.process_stereo(&mut stereo);
        let tail: Vec<f32> = stereo[stereo.len() - 200..].to_vec();
        let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
        assert!(rms < 0.1, "8kHz niet onderdrukt: rms = {}", rms);
    }
}
