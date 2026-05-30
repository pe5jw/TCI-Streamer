//! Classificeert binaire TCI frames als audio of IQ aan de hand van de
//! TCI text-commando's die we hebben gezien. Dit is nodig omdat Thetis
//! beide soorten data via dezelfde WebSocket stuurt zonder duidelijke marker.
//!
//! Strategie:
//!   - Hou bij wat de IQ samplerate is (uit `iq_sample_rate:` of default 192000)
//!   - Hou bij of `iq_start:0` actief is en `audio_start:0` actief is
//!   - Audio frames = klein (256-1024 stereo samples bij 48kHz)
//!   - IQ frames = groot (bij 192kHz veel meer samples per packet)
//!
//! Thetis stuurt een binair TCI frame met header. Het exacte formaat:
//!   [u32 receiver] [u32 sample_rate] [u32 format] [u32 codec] [u32 crc] [u32 length] [u32 type] [u32 reserved] [data...]
//!
//! Type field: 0=IQ, 1=RX_AUDIO, 2=TX_AUDIO_STREAM, 3=TX_CHRONO, 4=RX_CHRONO

use byteorder::{ByteOrder, LittleEndian};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TciStreamKind {
    Iq,
    RxAudio,
    TxAudioStream,
    TxChrono,
    RxChrono,
    Unknown,
}

/// Lees het TCI binary header en bepaal het stream type.
/// Returns (kind, sample_rate, sample_count, payload_offset).
///
/// **Sample-count komt uit de buffer-lengte**, niet uit het length veld
/// in de header — dat veld is niet betrouwbaar als sample-count.
/// Zie ook `read_f32_samples`.
pub fn classify_tci_binary(data: &[u8]) -> Option<(TciStreamKind, u32, u32, usize)> {
    // TCI binary header is 32 bytes
    if data.len() < 32 {
        return None;
    }
    let sample_rate = LittleEndian::read_u32(&data[4..8]);
    let stream_type = LittleEndian::read_u32(&data[24..28]);

    let kind = match stream_type {
        0 => TciStreamKind::Iq,
        1 => TciStreamKind::RxAudio,
        2 => TciStreamKind::TxAudioStream,
        3 => TciStreamKind::TxChrono,
        4 => TciStreamKind::RxChrono,
        _ => TciStreamKind::Unknown,
    };

    // Echte sample count = (totale lengte - 32 byte header) / 4 byte per float
    let payload_bytes = data.len() - 32;
    let sample_count = (payload_bytes / 4) as u32;

    Some((kind, sample_rate, sample_count, 32))
}

/// Bouw een TCI binary frame voor doorgifte aan een TCI client (op de
/// compactor-client kant). Ontvanger=0 voor RX0.
pub fn build_tci_binary(
    kind: TciStreamKind,
    receiver: u32,
    sample_rate: u32,
    samples_f32_interleaved: &[f32],
) -> Vec<u8> {
    let stream_type: u32 = match kind {
        TciStreamKind::Iq => 0,
        TciStreamKind::RxAudio => 1,
        TciStreamKind::TxAudioStream => 2,
        TciStreamKind::TxChrono => 3,
        TciStreamKind::RxChrono => 4,
        TciStreamKind::Unknown => 0,
    };
    let payload_bytes = samples_f32_interleaved.len() * 4;
    let mut buf = Vec::with_capacity(32 + payload_bytes);
    let mut hdr = [0u8; 32];
    LittleEndian::write_u32(&mut hdr[0..4], receiver);
    LittleEndian::write_u32(&mut hdr[4..8], sample_rate);
    LittleEndian::write_u32(&mut hdr[8..12], 3); // format=3 (float32)
    LittleEndian::write_u32(&mut hdr[12..16], 0); // codec=0 (PCM)
    LittleEndian::write_u32(&mut hdr[16..20], 0); // crc=0
    LittleEndian::write_u32(&mut hdr[20..24], samples_f32_interleaved.len() as u32);
    LittleEndian::write_u32(&mut hdr[24..28], stream_type);
    LittleEndian::write_u32(&mut hdr[28..32], 0); // reserved
    buf.extend_from_slice(&hdr);
    for &s in samples_f32_interleaved {
        let mut tmp = [0u8; 4];
        LittleEndian::write_f32(&mut tmp, s);
        buf.extend_from_slice(&tmp);
    }
    buf
}

/// Lees ALLE float32 IQ/audio samples uit een TCI binary frame payload.
///
/// **Belangrijk**: het `length` veld in de TCI header is NIET de
/// sample-count en kan kleiner zijn dan het werkelijk aantal samples
/// in de payload. We rekenen daarom de echte payload-grootte uit de
/// buffer-lengte, niet uit het header-veld. Anders verliezen we
/// systematisch samples op elke frame-grens, wat klikken met vast
/// tempo veroorzaakt in zowel audio als spectrum.
///
/// `_legacy_count` parameter wordt genegeerd; behouden voor compat.
pub fn read_f32_samples(data: &[u8], _legacy_count: u32) -> Vec<f32> {
    if data.len() <= 32 {
        return Vec::new();
    }
    let payload_bytes = data.len() - 32;
    let sample_count = payload_bytes / 4;
    let mut out = Vec::with_capacity(sample_count);
    let mut i = 32;
    let end = 32 + sample_count * 4;
    while i + 4 <= end {
        out.push(LittleEndian::read_f32(&data[i..i + 4]));
        i += 4;
    }
    out
}

/// Helper: extract het receiver index uit het header (0..3).
pub fn read_receiver(data: &[u8]) -> u32 {
    if data.len() < 4 {
        0
    } else {
        LittleEndian::read_u32(&data[0..4])
    }
}
