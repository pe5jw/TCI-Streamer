//! IQ stream codec — twee modes:
//!   - Spectrum: server doet FFT, stuurt alleen power-spectrum (max compressie)
//!   - DecimatedIq: lossy downsample naar lagere rate, int16 quantisatie

use rustfft::{num_complex::Complex32, FftPlanner};
use std::sync::Arc;

/// IQ-swap optie. Het verschil tussen een Zeus- en een Thetis-server zit
/// vaak in de IQ-conventie: de één levert het spectrum gespiegeld t.o.v. de
/// ander. Met deze optie corrigeer je dat zodat de waterfall niet gespiegeld
/// is.
///
///   - None: geen wijziging (I,Q blijven I,Q)
///   - SwapIQ: verwissel I en Q per sample → spiegelt het spectrum
///   - ConjQ: keer het teken van Q om (complex conjugaat) → spiegelt het
///     spectrum op een andere manier (rond DC)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IqSwap {
    None,
    SwapIQ,
    ConjQ,
}

impl IqSwap {
    /// Pas de swap in-place toe op interleaved IQ float32 samples (I,Q,I,Q,...).
    pub fn apply(self, iq: &mut [f32]) {
        match self {
            IqSwap::None => {}
            IqSwap::SwapIQ => {
                // Verwissel elk (I,Q) paar naar (Q,I).
                for pair in iq.chunks_exact_mut(2) {
                    pair.swap(0, 1);
                }
            }
            IqSwap::ConjQ => {
                // Keer het teken van Q (de imaginaire component) om.
                for pair in iq.chunks_exact_mut(2) {
                    pair[1] = -pair[1];
                }
            }
        }
    }
}

pub struct SpectrumProcessor {
    fft: Arc<dyn rustfft::Fft<f32>>,
    fft_size: usize,
    bin_count: usize,
    window: Vec<f32>,
    accum: Vec<Complex32>,
    avg_power: Vec<f32>,
    avg_alpha: f32,
    last_emit: std::time::Instant,
    emit_interval: std::time::Duration,
}

impl SpectrumProcessor {
    pub fn new(fft_size: usize, bin_count: usize, fps: u8) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        // Hann window voor minder spectral leakage
        let window: Vec<f32> = (0..fft_size)
            .map(|i| {
                let phi = 2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32;
                0.5 * (1.0 - phi.cos())
            })
            .collect();
        Self {
            fft,
            fft_size,
            bin_count: bin_count.min(fft_size),
            window,
            accum: Vec::with_capacity(fft_size),
            avg_power: vec![-120.0; fft_size],
            avg_alpha: 0.4,
            last_emit: std::time::Instant::now(),
            emit_interval: std::time::Duration::from_millis(1000 / fps.max(1) as u64),
        }
    }

    /// Voeg interleaved IQ float32 samples toe (I,Q,I,Q,...).
    /// Geeft Some(bins) terug als het tijd is om een spectrum-frame te emitten.
    pub fn push(&mut self, iq_samples: &[f32]) -> Option<SpectrumFrame> {
        // Pak (I,Q) paren als Complex32
        for chunk in iq_samples.chunks_exact(2) {
            self.accum.push(Complex32::new(chunk[0], chunk[1]));
            if self.accum.len() == self.fft_size {
                // Apply window
                for (s, w) in self.accum.iter_mut().zip(self.window.iter()) {
                    *s *= *w;
                }
                self.fft.process(&mut self.accum);

                // Compute power in dBFS, met exponentiële averaging voor smooth waterfall
                for (i, c) in self.accum.iter().enumerate() {
                    let p = (c.norm_sqr() / self.fft_size as f32).max(1e-20);
                    let db = 10.0 * p.log10();
                    self.avg_power[i] =
                        self.avg_power[i] * (1.0 - self.avg_alpha) + db * self.avg_alpha;
                }
                self.accum.clear();
            }
        }

        if self.last_emit.elapsed() >= self.emit_interval {
            self.last_emit = std::time::Instant::now();
            Some(self.build_frame())
        } else {
            None
        }
    }

    fn build_frame(&self) -> SpectrumFrame {
        // FFT shift: stop DC in het midden (negatieve freq links, positief rechts)
        let half = self.fft_size / 2;
        let shifted: Vec<f32> = self.avg_power[half..]
            .iter()
            .chain(self.avg_power[..half].iter())
            .copied()
            .collect();

        // Resample naar bin_count (linear interpolatie als nodig)
        let bins: Vec<f32> = if self.bin_count == self.fft_size {
            shifted
        } else {
            let ratio = self.fft_size as f32 / self.bin_count as f32;
            (0..self.bin_count)
                .map(|i| {
                    let src_idx = (i as f32 * ratio) as usize;
                    let src_end = (((i + 1) as f32 * ratio) as usize).min(self.fft_size);
                    // Max-hold over de groep — voor zwakke signalen behoud
                    shifted[src_idx..src_end.max(src_idx + 1)]
                        .iter()
                        .copied()
                        .fold(f32::MIN, f32::max)
                })
                .collect()
        };

        // Schaal naar u8 met -128..0 dBFS bereik (i8::MIN = -128).
        // 128 dB dynamisch bereik is meer dan genoeg voor radio toepassingen
        // waar typische ruisvloer rond -100 tot -120 dBFS ligt.
        let db_min: i8 = -128;
        let db_max: i8 = 0;
        let range = (db_max as i32 - db_min as i32) as f32;
        let bins_u8: Vec<u8> = bins
            .iter()
            .map(|&db| {
                let clamped = db.clamp(db_min as f32, db_max as f32);
                (((clamped - db_min as f32) / range) * 255.0) as u8
            })
            .collect();

        SpectrumFrame {
            db_min,
            db_max,
            bins_u8,
        }
    }
}

pub struct SpectrumFrame {
    pub db_min: i8,
    pub db_max: i8,
    pub bins_u8: Vec<u8>,
}

/// Decimator voor IqInt16 modus: filtert + downsamplet IQ van bron-samplerate
/// naar doel-samplerate, en converteert float32 → int16.
///
/// Gebruikt een simpele moving-average als anti-alias filter (snel, goed genoeg
/// voor onze use-case waar exacte filterresponse minder kritiek is dan latency).
pub struct IqDecimator {
    decim_factor: usize,
    accum_i: f32,
    accum_q: f32,
    counter: usize,
    target_rate: u32,
}

impl IqDecimator {
    pub fn new(source_rate: u32, target_rate: u32) -> Self {
        let factor = (source_rate / target_rate).max(1) as usize;
        Self {
            decim_factor: factor,
            accum_i: 0.0,
            accum_q: 0.0,
            counter: 0,
            target_rate: source_rate / factor as u32,
        }
    }

    pub fn target_rate(&self) -> u32 {
        self.target_rate
    }

    /// Push interleaved IQ float32 samples, return interleaved int16 samples.
    pub fn push(&mut self, iq_in: &[f32]) -> Vec<i16> {
        let mut out = Vec::with_capacity(iq_in.len() / self.decim_factor / 2 * 2);
        for chunk in iq_in.chunks_exact(2) {
            self.accum_i += chunk[0];
            self.accum_q += chunk[1];
            self.counter += 1;
            if self.counter >= self.decim_factor {
                let i_avg = self.accum_i / self.decim_factor as f32;
                let q_avg = self.accum_q / self.decim_factor as f32;
                out.push(float_to_i16(i_avg));
                out.push(float_to_i16(q_avg));
                self.accum_i = 0.0;
                self.accum_q = 0.0;
                self.counter = 0;
            }
        }
        out
    }
}

#[inline]
fn float_to_i16(f: f32) -> i16 {
    let clamped = f.clamp(-1.0, 1.0);
    (clamped * 32767.0) as i16
}

/// Inverse: int16 IQ → float32 IQ (voor client-side reconstructie).
pub fn i16_to_float_iq(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| s as f32 / 32767.0).collect()
}

/// Reconstrueer een TCI-compatibel IQ-frame vanuit een SpectrumU8 frame.
/// Niet perfect (we hebben de fase weggegooid), maar voldoende voor visuele
/// waterfall/spectrum weergave op de client.
///
/// We genereren een complex signaal waarvan de FFT magnitude overeenkomt
/// met de gegeven bins, met willekeurige fases (gauss white noise illusion).
pub fn synth_iq_from_spectrum(
    bins_u8: &[u8],
    db_min: i8,
    db_max: i8,
    out_samples: usize,
) -> Vec<f32> {
    use rustfft::FftPlanner;
    let n = bins_u8
        .len()
        .next_power_of_two()
        .max(out_samples.next_power_of_two());
    // i32 arithmetic om i8 overflow te voorkomen (db_max - db_min kan 128 zijn)
    let range = (db_max as i32 - db_min as i32) as f32;

    // Zet bins terug naar lineaire amplitude
    let mut spectrum: Vec<Complex32> = vec![Complex32::new(0.0, 0.0); n];
    for (i, &b) in bins_u8.iter().enumerate() {
        let db = db_min as f32 + (b as f32 / 255.0) * range;
        let amplitude = 10f32.powf(db / 20.0);
        // Pseudo-random fase op basis van bin-index voor reproduceerbaarheid
        let phase = (i as f32 * 2.39996323) % (2.0 * std::f32::consts::PI);
        // FFT shift terug: bin 0 in middle → bin 0 in array start
        let target_idx = (i + n / 2) % n;
        spectrum[target_idx] = Complex32::from_polar(amplitude, phase);
    }

    let mut planner = FftPlanner::<f32>::new();
    let ifft = planner.plan_fft_inverse(n);
    ifft.process(&mut spectrum);

    // Output als interleaved I,Q,I,Q...
    let scale = 1.0 / n as f32;
    spectrum
        .iter()
        .take(out_samples)
        .flat_map(|c| [c.re * scale, c.im * scale])
        .collect()
}
