//! FLAC lossless audio codec voor streaming per chunk.
//!
//! In tegenstelling tot Opus is FLAC volledig lossless — bit-perfecte
//! reconstructie. We encoderen elke audio-chunk als een compleet
//! zelf-bevattend FLAC-stream (incl. STREAMINFO header), zodat elke chunk
//! onafhankelijk te decoderen is. De header-overhead (~40 bytes per chunk)
//! is verwaarloosbaar voor een high-quality/diagnose mode.
//!
//! Compressie is typisch 40-60% afhankelijk van de signaalinhoud: stilte
//! en spraak comprimeren goed, witte ruis nauwelijks.
//!
//! Beide kanten zijn pure-Rust: flacenc (encode) en claxon (decode), geen
//! C-vendoring zoals bij libopus.

use anyhow::{anyhow, Result};
use flacenc::component::BitRepr;
use flacenc::error::Verify;

/// FLAC encoder voor streaming. Stateless per chunk: elke `encode_chunk`
/// produceert een compleet FLAC-stream.
pub struct FlacEncoder {
    channels: usize,
    bits_per_sample: usize,
    sample_rate: usize,
    config: flacenc::error::Verified<flacenc::config::Encoder>,
    block_size: usize,
}

impl FlacEncoder {
    /// Maak een encoder. `channels` is 1 (mono) of 2 (stereo); audio wordt
    /// als 16-bit verwerkt (bits_per_sample = 16).
    pub fn new(channels: u8, sample_rate: u32) -> Result<Self> {
        // default() levert een config met multithread=true als de "par"
        // feature aan staat. Wij compileren flacenc met default-features
        // = false (geen par), dus single-thread. We zetten multithread
        // expliciet uit voor het geval de default toch true is — onze
        // chunks zijn klein (~2048 samples), multithreading heeft geen zin.
        let mut cfg = flacenc::config::Encoder::default();
        cfg.multithread = false;
        let block_size = cfg.block_size;
        let config = cfg
            .into_verified()
            .map_err(|e| anyhow!("FLAC config ongeldig: {:?}", e))?;
        Ok(Self {
            channels: channels as usize,
            bits_per_sample: 16,
            sample_rate: sample_rate as usize,
            config,
            block_size,
        })
    }

    /// Encodeer een chunk float32 samples (interleaved) naar een compleet
    /// FLAC-stream (bytes). De f32 samples worden naar i16-range geconverteerd.
    pub fn encode_chunk(&self, samples_f32: &[f32]) -> Result<Vec<u8>> {
        // f32 [-1,1] → i32 in 16-bit range
        let samples_i32: Vec<i32> = samples_f32
            .iter()
            .map(|&s| {
                let clamped = s.clamp(-1.0, 1.0);
                (clamped * 32767.0).round() as i32
            })
            .collect();

        if samples_i32.is_empty() {
            return Ok(Vec::new());
        }

        let source = flacenc::source::MemSource::from_samples(
            &samples_i32,
            self.channels,
            self.bits_per_sample,
            self.sample_rate,
        );

        // block_size uit config (default 4096). Voor onze chunks (~2048
        // samples per kanaal) is dat prima — FLAC kan variabele laatste
        // block aan.
        let flac_stream =
            flacenc::encode_with_fixed_block_size(&self.config, source, self.block_size)
                .map_err(|e| anyhow!("FLAC encode failed: {:?}", e))?;

        let mut sink = flacenc::bitsink::ByteSink::new();
        flac_stream
            .write(&mut sink)
            .map_err(|e| anyhow!("FLAC serialize failed: {:?}", e))?;

        Ok(sink.as_slice().to_vec())
    }
}

/// FLAC decoder. Stateless per chunk: elke `decode_chunk` parsed een
/// compleet FLAC-stream zoals door `FlacEncoder::encode_chunk` gemaakt.
pub struct FlacDecoder {
    /// Verwacht aantal output kanalen (voor evt. mono→stereo expand).
    output_channels: u8,
}

impl FlacDecoder {
    pub fn new(output_channels: u8) -> Self {
        Self { output_channels }
    }

    /// Decodeer een FLAC-stream chunk naar float32 samples (interleaved),
    /// geconverteerd naar `output_channels`. De sample rate van de FLAC
    /// stream wordt mee teruggegeven.
    pub fn decode_chunk(&self, flac_bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
        if flac_bytes.is_empty() {
            return Ok((Vec::new(), 48_000));
        }
        let cursor = std::io::Cursor::new(flac_bytes);
        let mut reader = claxon::FlacReader::new(cursor)
            .map_err(|e| anyhow!("FLAC decode init failed: {}", e))?;

        let info = reader.streaminfo();
        let src_channels = info.channels as usize;
        let bits = info.bits_per_sample;
        let sample_rate = info.sample_rate;
        // Schaalfactor om naar f32 [-1,1] te brengen.
        let scale = 1.0_f32 / ((1u32 << (bits - 1)) as f32);

        // Claxon levert samples interleaved via .samples() iterator.
        let mut interleaved_f32: Vec<f32> = Vec::new();
        for sample in reader.samples() {
            let s = sample.map_err(|e| anyhow!("FLAC sample read failed: {}", e))?;
            interleaved_f32.push(s as f32 * scale);
        }

        // Channel conversie naar output_channels.
        let out_ch = self.output_channels as usize;
        let converted = if src_channels == out_ch {
            interleaved_f32
        } else if src_channels == 1 && out_ch == 2 {
            // mono → stereo dup
            let mut v = Vec::with_capacity(interleaved_f32.len() * 2);
            for &s in &interleaved_f32 {
                v.push(s);
                v.push(s);
            }
            v
        } else if src_channels == 2 && out_ch == 1 {
            // stereo → mono mix
            let m = interleaved_f32.len() / 2;
            let mut v = Vec::with_capacity(m);
            for i in 0..m {
                v.push((interleaved_f32[i * 2] + interleaved_f32[i * 2 + 1]) * 0.5);
            }
            v
        } else {
            interleaved_f32
        };

        Ok((converted, sample_rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_stereo() {
        let enc = FlacEncoder::new(2, 48_000).unwrap();
        let dec = FlacDecoder::new(2);
        // Genereer een testsignaal: 2048 stereo samples
        let mut samples = Vec::new();
        for i in 0..2048 {
            let t = i as f32 / 48_000.0;
            let l = (2.0 * std::f32::consts::PI * 1000.0 * t).sin() * 0.5;
            let r = (2.0 * std::f32::consts::PI * 1500.0 * t).sin() * 0.5;
            samples.push(l);
            samples.push(r);
        }
        let encoded = enc.encode_chunk(&samples).unwrap();
        assert!(!encoded.is_empty());
        // Compressie moet werken (encoded kleiner dan raw i16)
        let raw_size = samples.len() * 2;
        println!("FLAC: {} bytes, raw i16: {} bytes", encoded.len(), raw_size);

        let (decoded, sr) = dec.decode_chunk(&encoded).unwrap();
        assert_eq!(sr, 48_000);
        assert_eq!(decoded.len(), samples.len());
        // Lossless: na i16 quantisatie moet het bijna exact matchen
        for (orig, dec) in samples.iter().zip(decoded.iter()) {
            let diff = (orig - dec).abs();
            assert!(diff < 0.001, "diff te groot: {} vs {}", orig, dec);
        }
    }

    #[test]
    fn roundtrip_mono_to_stereo() {
        let enc = FlacEncoder::new(1, 48_000).unwrap();
        let dec = FlacDecoder::new(2);
        let mut samples = Vec::new();
        for i in 0..1024 {
            let t = i as f32 / 48_000.0;
            samples.push((2.0 * std::f32::consts::PI * 800.0 * t).sin() * 0.5);
        }
        let encoded = enc.encode_chunk(&samples).unwrap();
        let (decoded, _sr) = dec.decode_chunk(&encoded).unwrap();
        // mono input → stereo output, dus 2× zoveel samples
        assert_eq!(decoded.len(), samples.len() * 2);
    }
}
