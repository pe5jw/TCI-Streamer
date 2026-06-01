//! Opus audio codec wrapper voor RX/TX audio compressie.
//!
//! Werkt met `AudioConfig` die sample-rate, frame-duur, bitrate en
//! channels (mono/stereo) regelt. De encoder accepteert Thetis' input
//! (typisch 48kHz stereo) en resamplet + mixt naar de Opus codec
//! sample-rate en channels. De decoder doet het omgekeerde.

use crate::proto::{AudioChannels, AudioConfig};
use anyhow::{anyhow, Result};
use opus::{Application, Channels, Decoder as OpusDecoder, Encoder as OpusEncoder};

pub struct AudioEncoder {
    encoder: OpusEncoder,
    config: AudioConfig,
    /// Buffer met samples op de codec sample-rate en channel-count.
    pcm_buf: Vec<f32>,
    /// Input sample rate (van Thetis, typisch 48kHz).
    input_rate: u32,
    /// Aantal kanalen in de input (van Thetis, typisch 2).
    input_channels: u8,
    /// Resample state: fractional positie in de input buffer.
    resample_pos: f64,
}

impl AudioEncoder {
    /// Maak een encoder. `input_rate` en `input_channels` beschrijven wat
    /// `push()` krijgt; de encoder doet zelf resampling en mono-downmix
    /// als de codec-config anders is.
    pub fn new(config: AudioConfig, input_channels: u8, input_rate: u32) -> Result<Self> {
        config.validate()?;
        let opus_channels = match config.channels {
            AudioChannels::Mono => Channels::Mono,
            AudioChannels::Stereo => Channels::Stereo,
        };
        // Voor lage bitrates is Voip beter, anders Audio.
        let app = if config.bitrate <= 32_000 {
            Application::Voip
        } else {
            Application::Audio
        };
        let mut encoder = OpusEncoder::new(config.sample_rate, opus_channels, app)
            .map_err(|e| anyhow!("Opus encoder init failed: {}", e))?;
        encoder
            .set_bitrate(opus::Bitrate::Bits(config.bitrate as i32))
            .map_err(|e| anyhow!("Opus set_bitrate failed: {}", e))?;
        Ok(Self {
            encoder,
            config,
            pcm_buf: Vec::with_capacity(
                config.frame_samples_per_channel() * config.channels.count() as usize * 2,
            ),
            input_rate,
            input_channels,
            resample_pos: 0.0,
        })
    }

    /// Codec sample rate (= wat in de Opus-packets zit).
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    /// Codec channel count.
    pub fn channels(&self) -> u8 {
        self.config.channels.count()
    }

    /// Voeg input samples toe en geef volledige Opus-frames terug.
    /// `samples` is interleaved op `input_rate` × `input_channels`.
    pub fn push(&mut self, samples: &[f32]) -> Result<Vec<Vec<u8>>> {
        // Stap 1: converteer kanaal-count indien nodig (stereo→mono mix).
        let after_channels: std::borrow::Cow<'_, [f32]> =
            if self.input_channels == 2 && self.config.channels == AudioChannels::Mono {
                // L+R gemiddelde
                let n = samples.len() / 2;
                let mut mono = Vec::with_capacity(n);
                for i in 0..n {
                    let l = samples[i * 2];
                    let r = samples[i * 2 + 1];
                    mono.push((l + r) * 0.5);
                }
                std::borrow::Cow::Owned(mono)
            } else if self.input_channels == 1 && self.config.channels == AudioChannels::Stereo {
                // Mono → stereo dup
                let mut st = Vec::with_capacity(samples.len() * 2);
                for &s in samples {
                    st.push(s);
                    st.push(s);
                }
                std::borrow::Cow::Owned(st)
            } else {
                std::borrow::Cow::Borrowed(samples)
            };

        // Stap 2: resample naar codec sample_rate als nodig.
        if self.input_rate != self.config.sample_rate {
            self.push_resampled(&after_channels);
        } else {
            self.pcm_buf.extend_from_slice(&after_channels);
        }

        // Stap 3: knip in Opus frames.
        let ch = self.config.channels.count() as usize;
        let frame_size = self.config.frame_samples_per_channel() * ch;
        let mut out = Vec::new();
        while self.pcm_buf.len() >= frame_size {
            let frame: Vec<f32> = self.pcm_buf.drain(..frame_size).collect();
            // Max Opus payload ~4000 bytes; 1500 ruim voldoende voor onze bitrates.
            let mut compressed = vec![0u8; 1500];
            let n = self
                .encoder
                .encode_float(&frame, &mut compressed)
                .map_err(|e| anyhow!("Opus encode failed: {}", e))?;
            compressed.truncate(n);
            out.push(compressed);
        }
        Ok(out)
    }

    /// Lineaire resampling van input_rate naar config.sample_rate.
    /// `samples` is op input_rate, in config.channels layout.
    fn push_resampled(&mut self, samples: &[f32]) {
        let ch = self.config.channels.count() as usize;
        let ratio = self.input_rate as f64 / self.config.sample_rate as f64;
        if samples.is_empty() || ch == 0 {
            return;
        }
        let frame_count = samples.len() / ch;
        while self.resample_pos + 1.0 < frame_count as f64 {
            let i = self.resample_pos as usize;
            let frac = (self.resample_pos - i as f64) as f32;
            for c in 0..ch {
                let s0 = samples[i * ch + c];
                let s1 = samples[(i + 1) * ch + c];
                let interpolated = s0 + (s1 - s0) * frac;
                self.pcm_buf.push(interpolated);
            }
            self.resample_pos += ratio;
        }
        self.resample_pos -= frame_count as f64;
        if self.resample_pos < 0.0 {
            self.resample_pos = 0.0;
        }
    }
}

pub struct AudioDecoder {
    decoder: OpusDecoder,
    /// Codec channel count (uit AudioConfig).
    codec_channels: u8,
    /// Codec sample rate.
    codec_sample_rate: u32,
    pcm_scratch: Vec<f32>,
    /// Output sample rate (van Thetis/THRA, typisch 48kHz). Decoder resampled
    /// naar deze rate.
    output_rate: u32,
    /// Output channel count (typisch 2 voor stereo TCI). Decoder dupliceert
    /// mono naar stereo als nodig.
    output_channels: u8,
    /// Resample state.
    resample_pos: f64,
    /// Vorige laatste sample per kanaal voor de resampler (zodat de
    /// laatste sample van vorige frame correct geïnterpoleerd kan worden
    /// met de eerste van de nieuwe frame).
    prev_sample: Vec<f32>,
}

impl AudioDecoder {
    /// Maak een decoder. `codec_channels` en `codec_sample_rate` matchen
    /// de encoder; `output_channels` en `output_rate` bepalen het formaat
    /// dat `decode()` retourneert.
    pub fn new(
        codec_channels: u8,
        codec_sample_rate: u32,
        output_channels: u8,
        output_rate: u32,
    ) -> Result<Self> {
        let opus_channels = match codec_channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            n => return Err(anyhow!("Onverwacht aantal kanalen: {}", n)),
        };
        let decoder = OpusDecoder::new(codec_sample_rate, opus_channels)
            .map_err(|e| anyhow!("Opus decoder init failed: {}", e))?;
        let pcm_scratch = vec![0.0f32; 5760 * codec_channels as usize];
        Ok(Self {
            decoder,
            codec_channels,
            codec_sample_rate,
            pcm_scratch,
            output_rate,
            output_channels,
            resample_pos: 0.0,
            prev_sample: vec![0.0; codec_channels as usize],
        })
    }

    /// Decodeer een Opus-frame en retourneer samples in de output layout.
    /// Het resultaat is interleaved op `output_rate` met `output_channels`.
    pub fn decode(&mut self, opus_bytes: &[u8]) -> Result<Vec<f32>> {
        let n = self
            .decoder
            .decode_float(opus_bytes, &mut self.pcm_scratch[..], false)
            .map_err(|e| anyhow!("Opus decode failed: {}", e))?;
        let codec_ch = self.codec_channels as usize;
        let total = n * codec_ch;
        let codec_samples = self.pcm_scratch[..total].to_vec();

        // Stap 1: resample naar output_rate (in codec channels).
        let intermediate: Vec<f32> = if self.codec_sample_rate == self.output_rate {
            codec_samples.clone()
        } else {
            let ratio = self.codec_sample_rate as f64 / self.output_rate as f64;
            let mut out = Vec::with_capacity((n as f64 / ratio) as usize * codec_ch + 16);
            let mut pos = self.resample_pos;
            while pos + 1.0 < n as f64 {
                let i_f = pos;
                let i = i_f.floor() as i32;
                let frac = (i_f - i as f64) as f32;
                for c in 0..codec_ch {
                    let s0 = if i < 0 {
                        self.prev_sample[c]
                    } else {
                        codec_samples[i as usize * codec_ch + c]
                    };
                    let s1 = codec_samples[(i + 1) as usize * codec_ch + c];
                    out.push(s0 + (s1 - s0) * frac);
                }
                pos += ratio;
            }
            self.resample_pos = pos - n as f64;
            if n > 0 {
                for c in 0..codec_ch {
                    self.prev_sample[c] = codec_samples[(n - 1) * codec_ch + c];
                }
            }
            out
        };

        // Stap 2: channel conversie.
        let out_ch = self.output_channels as usize;
        let final_samples: Vec<f32> = if codec_ch == out_ch {
            intermediate
        } else if codec_ch == 1 && out_ch == 2 {
            let mut v = Vec::with_capacity(intermediate.len() * 2);
            for &s in &intermediate {
                v.push(s);
                v.push(s);
            }
            v
        } else if codec_ch == 2 && out_ch == 1 {
            let m = intermediate.len() / 2;
            let mut v = Vec::with_capacity(m);
            for i in 0..m {
                v.push((intermediate[i * 2] + intermediate[i * 2 + 1]) * 0.5);
            }
            v
        } else {
            intermediate
        };

        Ok(final_samples)
    }

    pub fn codec_channels(&self) -> u8 {
        self.codec_channels
    }
}
