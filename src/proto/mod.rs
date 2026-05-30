//! Wire protocol tussen tci-compactor-server en tci-compactor-client.
//!
//! Elke WebSocket message is één frame. De eerste byte is het frame type,
//! de rest is payload. Tekst-commando's zijn UTF-8, binaire payloads zijn
//! length-prefixed of self-describing.
//!
//! Frame layout:
//!   [u8 frame_type] [payload...]

pub mod tci;

use anyhow::{anyhow, Result};
use byteorder::{ByteOrder, LittleEndian};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// TCI tekst-commando, 1:1 doorgegeven (zowel client→server als server→client).
    /// Payload: UTF-8 bytes, geen length prefix (rest van het frame).
    TciText = 0x01,

    /// Opus-gecomprimeerde RX audio (server → client).
    /// Payload: [u32 sample_rate] [u8 channels] [u8 _reserved] [u16 _reserved] [opus_bytes...]
    OpusRxAudio = 0x10,

    /// Opus-gecomprimeerde TX audio (client → server).
    /// Payload: zelfde layout als OpusRxAudio.
    OpusTxAudio = 0x11,

    /// Lossless RX audio (server → client) — geen Opus, voor diagnose/test.
    /// Payload: [u32 sample_rate] [u8 channels] [u8 format] [u16 sample_count]
    ///          [samples...]
    /// format: 0=float32 (4 bytes/sample), 1=int16 (2 bytes/sample).
    /// Sample-count is het aantal stereo-frames (dus aantal samples per kanaal).
    RawRxAudio = 0x12,

    /// Lossless TX audio (client → server). Zelfde layout.
    RawTxAudio = 0x13,

    /// FLAC-gecomprimeerde RX audio (server → client).
    /// Payload: [u32 sample_rate] [u8 channels] [u8 _reserved] [u16 _reserved] [flac_stream_bytes...]
    /// flac_stream_bytes is een compleet zelf-bevattend FLAC-stream
    /// (incl. STREAMINFO header) voor deze ene chunk.
    FlacRxAudio = 0x14,

    /// FLAC-gecomprimeerde TX audio (client → server). Zelfde layout.
    FlacTxAudio = 0x15,

    /// Spectrum frame — server-side FFT modus (server → client).
    /// Payload: [u32 center_hz] [u32 span_hz] [u16 bin_count] [u8 db_min] [u8 db_max] [u8 bins[bin_count]...]
    /// Elke bin is een u8 die geschaald is tussen db_min en db_max (signed dBFS).
    SpectrumU8 = 0x20,

    /// Spectrum frame — hogere precisie (server → client).
    /// Payload: [u32 center_hz] [u32 span_hz] [u16 bin_count] [u16 bins[bin_count]...]
    /// Elke bin is een i16 met dBFS * 100.
    SpectrumI16 = 0x21,

    /// Gedecimeerde IQ stream als int16 (server → client).
    /// Payload: [u32 sample_rate] [u32 sample_count] [i16 samples[sample_count*2]...]
    /// Stereo interleaved (I,Q,I,Q,...). Schaal: ±32767 = ±1.0 float.
    IqInt16 = 0x30,

    /// Heartbeat / keepalive (beide richtingen). Geen payload.
    Heartbeat = 0xF0,

    /// Capability negotiation — eerste message na connect (client → server).
    /// Payload: JSON met client capabilities en gewenste modes.
    Hello = 0xF1,

    /// Server's antwoord op Hello met geselecteerde modes.
    HelloAck = 0xF2,

    /// Diagnostic passthrough: ruwe TCI binary frame 1:1 doorgegeven
    /// (server → client). Payload: het ongewijzigde TCI binary frame
    /// inclusief 32-byte header. Voor debugging, bandbreedte niet
    /// gereduceerd.
    RawTciBinary = 0xFE,
}

impl FrameType {
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x01 => Ok(Self::TciText),
            0x10 => Ok(Self::OpusRxAudio),
            0x11 => Ok(Self::OpusTxAudio),
            0x12 => Ok(Self::RawRxAudio),
            0x13 => Ok(Self::RawTxAudio),
            0x14 => Ok(Self::FlacRxAudio),
            0x15 => Ok(Self::FlacTxAudio),
            0x20 => Ok(Self::SpectrumU8),
            0x21 => Ok(Self::SpectrumI16),
            0x30 => Ok(Self::IqInt16),
            0xF0 => Ok(Self::Heartbeat),
            0xF1 => Ok(Self::Hello),
            0xF2 => Ok(Self::HelloAck),
            0xFE => Ok(Self::RawTciBinary),
            _ => Err(anyhow!("Onbekend frame type: 0x{:02x}", v)),
        }
    }
}

/// Hello bericht — onderhandelt over modes per stream.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Hello {
    pub protocol_version: u32,
    pub iq_mode: IqMode,
    /// Audio codec en parameters. Sync van client naar server: server
    /// gebruikt deze instellingen voor RX-encoding en TX-decoding.
    pub audio: AudioConfig,
    pub spectrum_fps: u8,
    pub spectrum_bins: u16,
    /// Filter cutoffs voor RX/TX (Hz). 0 = uit. Server en client gebruiken
    /// beide deze waarden zodat het filter dezelfde plaats heeft in beide
    /// kanten van de pijplijn.
    pub rx_hp_hz: u16,
    pub rx_lp_hz: u16,
    pub tx_hp_hz: u16,
    pub tx_lp_hz: u16,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IqMode {
    /// Server doet FFT, stuurt SpectrumU8/I16 frames.
    /// Client moet IQ-frames synthetiseren of spectrum direct gebruiken.
    Spectrum,
    /// Server stuurt gedecimeerde int16 IQ-data (IqInt16 frames).
    /// Client decodeert terug naar float32 voor TCI passthrough.
    DecimatedIq,
    /// Geen IQ data doorsturen (bespaart alle IQ bandbreedte).
    Disabled,
}

/// Welk codec gebruikt wordt voor audio.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioCodec {
    /// Opus gecomprimeerd (lossy). Sample-rate/frame-ms/bitrate/channels
    /// uit `AudioConfig` worden gebruikt.
    Opus,
    /// FLAC gecomprimeerd (lossless). Bit-perfecte reconstructie bij
    /// ~40-60% van de originele grootte. Gebruikt sample_rate en channels
    /// uit `AudioConfig`; bitrate/frame_dms worden genegeerd (FLAC bepaalt
    /// z'n eigen blokgrootte). ~900 kbit/s typisch.
    Flac,
    /// Lossless float32 passthrough, ~3 Mbit/s. Voor diagnose. Geen
    /// resampling, geen channel-conversie — de audio van Thetis gaat 1:1
    /// door. De `AudioConfig` velden worden genegeerd.
    Lossless,
    /// Lossless int16, ~1.5 Mbit/s. Alleen bit-depth reductie. Geen
    /// resampling, geen channel-conversie.
    LosslessInt16,
}

/// Stereo / mono.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioChannels {
    Mono,
    Stereo,
}

impl AudioChannels {
    pub fn count(&self) -> u8 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
        }
    }
}

/// Audio codec configuratie. Sync van client naar server via Hello.
/// Geldt voor RX (server→client) én TX (client→server) — zelfde codec
/// settings voor beide richtingen.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AudioConfig {
    pub codec: AudioCodec,
    /// Opus interne sample rate. Toegestaan: 8000, 12000, 16000, 24000, 48000.
    /// Server downsamplet Thetis' 48kHz naar deze waarde voor RX, client
    /// upsampled terug naar 48kHz voor THRA. Voor TX andersom.
    pub sample_rate: u32,
    /// Frame duur in tienden van een ms (om 2.5ms representatief te hebben
    /// als integer). Toegestaan: 25, 50, 100, 200, 400, 600.
    pub frame_dms: u16,
    /// Bitrate in bits/sec. Typisch 8000-128000.
    pub bitrate: u32,
    pub channels: AudioChannels,
}

impl AudioConfig {
    /// Default: 24 kbit/s, mono, 20ms, 48kHz, Opus.
    pub fn default_opus() -> Self {
        Self {
            codec: AudioCodec::Opus,
            sample_rate: 48_000,
            frame_dms: 200,
            bitrate: 24_000,
            channels: AudioChannels::Mono,
        }
    }

    /// Default voor lossless modes (sample-rate/frame_dms/bitrate/channels
    /// zijn niet relevant maar moeten een geldige waarde hebben).
    pub fn lossless() -> Self {
        Self {
            codec: AudioCodec::Lossless,
            sample_rate: 48_000,
            frame_dms: 200,
            bitrate: 0,
            channels: AudioChannels::Stereo,
        }
    }

    pub fn lossless_int16() -> Self {
        Self {
            codec: AudioCodec::LosslessInt16,
            sample_rate: 48_000,
            frame_dms: 200,
            bitrate: 0,
            channels: AudioChannels::Stereo,
        }
    }

    /// FLAC config. Gebruikt sample_rate en channels; frame_dms en bitrate
    /// zijn niet relevant (FLAC kiest z'n eigen blokgrootte). Default mono
    /// 48kHz — voor SSB de logische keuze, halveert t.o.v. stereo.
    pub fn flac(sample_rate: u32, channels: AudioChannels) -> Self {
        Self {
            codec: AudioCodec::Flac,
            sample_rate,
            frame_dms: 200,
            bitrate: 0,
            channels,
        }
    }

    /// True als Opus daadwerkelijk gebruikt wordt.
    pub fn uses_opus(&self) -> bool {
        matches!(self.codec, AudioCodec::Opus)
    }

    /// True als FLAC gebruikt wordt.
    pub fn uses_flac(&self) -> bool {
        matches!(self.codec, AudioCodec::Flac)
    }

    /// Frame duur in ms (kan een halve ms zijn voor 2.5ms = 25 dms).
    pub fn frame_ms(&self) -> f32 {
        self.frame_dms as f32 / 10.0
    }

    /// Aantal samples per frame bij de codec sample rate, per kanaal.
    pub fn frame_samples_per_channel(&self) -> usize {
        (self.sample_rate as usize * self.frame_dms as usize) / 10_000
    }

    /// Validatie: returns error string als de combinatie niet geldig is.
    pub fn validate(&self) -> Result<()> {
        if self.codec == AudioCodec::Flac {
            // FLAC ondersteunt veel sample rates, maar wij downsamplen
            // niet voor FLAC (we sturen Thetis' 48kHz door). Dus alleen
            // 48000 toegestaan om resampling-complexiteit te vermijden.
            if self.sample_rate != 48_000 {
                return Err(anyhow!(
                    "FLAC mode ondersteunt alleen sample-rate 48000 (geen resampling); kreeg {}",
                    self.sample_rate
                ));
            }
            return Ok(());
        }
        if self.codec != AudioCodec::Opus {
            return Ok(());
        }
        match self.sample_rate {
            8_000 | 12_000 | 16_000 | 24_000 | 48_000 => {}
            _ => {
                return Err(anyhow!(
                    "Sample rate {} niet ondersteund door Opus (kies 8k/12k/16k/24k/48k)",
                    self.sample_rate
                ))
            }
        }
        match self.frame_dms {
            25 | 50 | 100 | 200 | 400 | 600 => {}
            _ => {
                return Err(anyhow!(
                    "Frame duur {}ms niet ondersteund door Opus (kies 2.5/5/10/20/40/60)",
                    self.frame_ms()
                ))
            }
        }
        if self.bitrate < 6_000 || self.bitrate > 510_000 {
            return Err(anyhow!(
                "Bitrate {} bps buiten Opus range (6000-510000)",
                self.bitrate
            ));
        }
        Ok(())
    }
}

/// Build een TCI text frame.
pub fn build_tci_text(text: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + text.len());
    buf.push(FrameType::TciText as u8);
    buf.extend_from_slice(text.as_bytes());
    buf
}

/// Build een Opus audio frame.
pub fn build_opus_audio(
    is_tx: bool,
    sample_rate: u32,
    channels: u8,
    opus_bytes: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 8 + opus_bytes.len());
    buf.push(if is_tx {
        FrameType::OpusTxAudio as u8
    } else {
        FrameType::OpusRxAudio as u8
    });
    let mut hdr = [0u8; 8];
    LittleEndian::write_u32(&mut hdr[0..4], sample_rate);
    hdr[4] = channels;
    // hdr[5..8] gereserveerd
    buf.extend_from_slice(&hdr);
    buf.extend_from_slice(opus_bytes);
    buf
}

/// Build een FLAC audio frame. `flac_bytes` is een compleet zelf-bevattend
/// FLAC-stream voor deze chunk. Header layout is identiek aan Opus.
pub fn build_flac_audio(
    is_tx: bool,
    sample_rate: u32,
    channels: u8,
    flac_bytes: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 8 + flac_bytes.len());
    buf.push(if is_tx {
        FrameType::FlacTxAudio as u8
    } else {
        FrameType::FlacRxAudio as u8
    });
    let mut hdr = [0u8; 8];
    LittleEndian::write_u32(&mut hdr[0..4], sample_rate);
    hdr[4] = channels;
    buf.extend_from_slice(&hdr);
    buf.extend_from_slice(flac_bytes);
    buf
}

/// Format-tag in een RawRxAudio/RawTxAudio frame.
pub const RAW_AUDIO_FORMAT_FLOAT32: u8 = 0;
pub const RAW_AUDIO_FORMAT_INT16: u8 = 1;

/// Build een lossless audio frame (float32 of int16).
/// `samples_interleaved` bevat stereo-interleaved samples (L,R,L,R,...).
/// Voor float32 zijn dat 4 bytes per sample; voor int16 zijn dat 2 bytes per sample.
pub fn build_raw_audio(
    is_tx: bool,
    sample_rate: u32,
    channels: u8,
    format: u8,
    samples_per_channel: u16,
    sample_bytes: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 8 + sample_bytes.len());
    buf.push(if is_tx {
        FrameType::RawTxAudio as u8
    } else {
        FrameType::RawRxAudio as u8
    });
    let mut hdr = [0u8; 8];
    LittleEndian::write_u32(&mut hdr[0..4], sample_rate);
    hdr[4] = channels;
    hdr[5] = format;
    LittleEndian::write_u16(&mut hdr[6..8], samples_per_channel);
    buf.extend_from_slice(&hdr);
    buf.extend_from_slice(sample_bytes);
    buf
}

/// Geparseerd lossless audio frame.
pub struct RawAudioFrame<'a> {
    pub sample_rate: u32,
    pub channels: u8,
    pub format: u8,
    pub samples_per_channel: u16,
    pub sample_bytes: &'a [u8],
}

/// Parse een RawRxAudio/RawTxAudio payload (de 8-byte header + samples,
/// zonder de leading FrameType byte).
pub fn parse_raw_audio(payload: &[u8]) -> Result<RawAudioFrame<'_>> {
    if payload.len() < 8 {
        return Err(anyhow!("Raw audio frame te kort: {} bytes", payload.len()));
    }
    let sample_rate = LittleEndian::read_u32(&payload[0..4]);
    let channels = payload[4];
    let format = payload[5];
    let samples_per_channel = LittleEndian::read_u16(&payload[6..8]);
    Ok(RawAudioFrame {
        sample_rate,
        channels,
        format,
        samples_per_channel,
        sample_bytes: &payload[8..],
    })
}

/// Build een SpectrumU8 frame.
pub fn build_spectrum_u8(
    center_hz: u32,
    span_hz: u32,
    db_min: i8,
    db_max: i8,
    bins_u8: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 12 + bins_u8.len());
    buf.push(FrameType::SpectrumU8 as u8);
    let mut hdr = [0u8; 12];
    LittleEndian::write_u32(&mut hdr[0..4], center_hz);
    LittleEndian::write_u32(&mut hdr[4..8], span_hz);
    LittleEndian::write_u16(&mut hdr[8..10], bins_u8.len() as u16);
    hdr[10] = db_min as u8;
    hdr[11] = db_max as u8;
    buf.extend_from_slice(&hdr);
    buf.extend_from_slice(bins_u8);
    buf
}

/// Build een gedecimeerde IQ int16 frame.
pub fn build_iq_int16(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let count = samples.len() / 2; // stereo
    let mut buf = Vec::with_capacity(1 + 8 + samples.len() * 2);
    buf.push(FrameType::IqInt16 as u8);
    let mut hdr = [0u8; 8];
    LittleEndian::write_u32(&mut hdr[0..4], sample_rate);
    LittleEndian::write_u32(&mut hdr[4..8], count as u32);
    buf.extend_from_slice(&hdr);
    for &s in samples {
        let mut tmp = [0u8; 2];
        LittleEndian::write_i16(&mut tmp, s);
        buf.extend_from_slice(&tmp);
    }
    buf
}

/// Parse een inkomend frame: (type, payload).
pub fn parse_frame(data: &[u8]) -> Result<(FrameType, &[u8])> {
    if data.is_empty() {
        return Err(anyhow!("Leeg frame"));
    }
    let ft = FrameType::from_u8(data[0])?;
    Ok((ft, &data[1..]))
}
