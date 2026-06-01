//! tci-streamer-client
//!
//! Draait op de remote PC. Verbindt met een tci-streamer-server, en presenteert
//! zelf een gewone TCI-server op 127.0.0.1:40001 (of andere poort) waar THRA,
//! N1MM, JTDX, of andere TCI-clients verbinding mee kunnen maken.

use anyhow::{anyhow, Context, Result};
use byteorder::{ByteOrder, LittleEndian};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tci_streamer::codec::audio::{AudioDecoder, AudioEncoder};
use tci_streamer::codec::filter::BandpassFilter;
use tci_streamer::codec::flac::{FlacDecoder, FlacEncoder};
use tci_streamer::codec::iq::{i16_to_float_iq, synth_iq_from_spectrum};
use tci_streamer::proto::tci::{build_tci_binary, TciStreamKind};
use tci_streamer::proto::{
    parse_frame, AudioChannels, AudioCodec, AudioConfig, FrameType, Hello, IqMode,
};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

#[derive(Parser, Debug, Clone)]
#[command(name = "tci-streamer-client")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "TCI streamer client - presents local TCI server")]
struct Args {
    /// Remote tci-streamer-server URL
    #[arg(long)]
    server: String,

    /// Listen address voor lokale TCI clients (THRA, N1MM, ...)
    #[arg(long, default_value = "127.0.0.1:40001")]
    listen: SocketAddr,

    /// IQ compression mode
    #[arg(long, value_enum, default_value = "spectrum")]
    iq_mode: CliIqMode,

    /// Audio codec keuze.
    ///   - opus: Opus lossy encoding (instelbaar via --sample-rate, --frame-ms, --bitrate, --channels)
    ///   - flac: FLAC lossless (~900 kbit/s), gebruikt --channels; bit-perfect
    ///   - lossless: float32 1:1 passthrough (~3 Mbit/s), voor diagnose
    ///   - lossless-int16: int16 passthrough (~1.5 Mbit/s), voor diagnose
    #[arg(long, value_enum, default_value = "opus")]
    codec: CliCodec,

    /// Opus interne sample rate (Hz). Toegestaan: 8000, 12000, 16000, 24000, 48000.
    /// Lagere waarden geven smallere audio-bandbreedte. Default 48000.
    #[arg(long, default_value = "48000")]
    sample_rate: u32,

    /// Opus frame duur in ms. Toegestaan: 2.5, 5, 10, 20, 40, 60.
    /// Default 20 (sweet spot voor latency/efficiency).
    #[arg(long, default_value = "20")]
    frame_ms: f32,

    /// Opus bitrate in bits/sec. Typisch 16000-48000 voor SSB. Default 24000.
    #[arg(long, default_value = "24000")]
    bitrate: u32,

    /// Audio kanalen. Default mono (voor SSB) — halveert bandbreedte t.o.v. stereo.
    #[arg(long, value_enum, default_value = "mono")]
    channels: CliChannels,

    /// Spectrum bins (alleen bij iq-mode=spectrum)
    #[arg(long, default_value = "2048")]
    spectrum_bins: u16,

    /// Spectrum FPS
    #[arg(long, default_value = "20")]
    spectrum_fps: u8,

    /// RX audio bandpass filter preset. Wordt op zowel server (vóór Opus
    /// encoding) als client (na Opus decoding) toegepast.
    #[arg(long, value_enum, default_value = "off")]
    rx_filter: CliFilterPreset,

    /// TX audio bandpass filter preset. Wordt op zowel client (vóór Opus
    /// encoding) als server (na Opus decoding) toegepast.
    #[arg(long, value_enum, default_value = "off")]
    tx_filter: CliFilterPreset,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum CliCodec {
    Opus,
    Flac,
    Lossless,
    /// Expliciete naam zodat de CLI-waarde ondubbelzinnig "lossless-int16" is
    /// (clap's default kebab-conversie zou er "lossless-int-16" van maken).
    #[value(name = "lossless-int16")]
    LosslessInt16,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum CliChannels {
    Mono,
    Stereo,
}

impl From<CliChannels> for AudioChannels {
    fn from(v: CliChannels) -> Self {
        match v {
            CliChannels::Mono => AudioChannels::Mono,
            CliChannels::Stereo => AudioChannels::Stereo,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum CliFilterPreset {
    /// Geen filter (bypass).
    Off,
    /// Breed 100-6000 Hz (muziek/data).
    Wide,
    /// Spraak 100-3000 Hz.
    Voice,
    /// SSB standaard 150-2800 Hz.
    Ssb,
    /// Smal 200-2800 Hz.
    Narrow,
}

impl From<CliFilterPreset> for tci_streamer::codec::filter::AudioFilterPreset {
    fn from(v: CliFilterPreset) -> Self {
        use tci_streamer::codec::filter::AudioFilterPreset as P;
        match v {
            CliFilterPreset::Off => P::Off,
            CliFilterPreset::Wide => P::Wide,
            CliFilterPreset::Voice => P::Voice,
            CliFilterPreset::Ssb => P::Ssb,
            CliFilterPreset::Narrow => P::Narrow,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum CliIqMode {
    Spectrum,
    DecimatedIq,
    Disabled,
}
impl From<CliIqMode> for IqMode {
    fn from(v: CliIqMode) -> Self {
        match v {
            CliIqMode::Spectrum => IqMode::Spectrum,
            CliIqMode::DecimatedIq => IqMode::DecimatedIq,
            CliIqMode::Disabled => IqMode::Disabled,
        }
    }
}

#[derive(Clone, Debug)]
enum DownEvent {
    Text(String),
    Binary(Arc<Vec<u8>>),
}

#[derive(Debug, Clone)]
enum UpEvent {
    Text(String),
    /// TX audio binary frame van een lokale TCI client (incl. TCI binary header)
    TxAudioBinary(Vec<u8>),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tci_streamer=info".parse()?)
                .add_directive("tci_streamer_client=info".parse()?),
        )
        .init();

    let args = Args::parse();
    info!("TCI Streamer Client v{} gestart", env!("CARGO_PKG_VERSION"));
    info!("  Server: {}", args.server);
    info!("  Listen: {}", args.listen);
    info!("  IQ:     {:?}", args.iq_mode);
    info!(
        "  Audio:  codec={:?} {}Hz {:.1}ms {}bps {:?}",
        args.codec, args.sample_rate, args.frame_ms, args.bitrate, args.channels
    );

    let (down_tx, _) = broadcast::channel::<DownEvent>(1024);
    let (up_tx, up_rx) = mpsc::channel::<UpEvent>(256);
    let up_rx_shared = Arc::new(Mutex::new(up_rx));

    {
        let down = down_tx.clone();
        let args = args.clone();
        let up_rx = up_rx_shared.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = run_server_link(&args, down.clone(), up_rx.clone()).await {
                    warn!("Server link verbroken: {:#}. Reconnect over 3s...", e);
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });
    }

    let listener = TcpListener::bind(args.listen)
        .await
        .with_context(|| format!("Kon niet luisteren op {}", args.listen))?;
    info!("Lokale TCI server listening op {}", args.listen);

    loop {
        let (stream, peer) = listener.accept().await?;
        // TCP_NODELAY: stuur kleine TCI-commando's en audio-frames meteen
        // door zonder Nagle-buffering. Verlaagt latency en voorkomt dat
        // kleine pakketten worden opgespaard tot grote (beter voor jitter).
        if let Err(e) = stream.set_nodelay(true) {
            warn!("Kon TCP_NODELAY niet zetten op {}: {}", peer, e);
        }
        info!("TCI client verbonden: {}", peer);
        let down_rx = down_tx.subscribe();
        let up_tx2 = up_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_local_tci_client(stream, down_rx, up_tx2).await {
                warn!("Lokale client {} loskoppeld: {:#}", peer, e);
            }
        });
    }
}

async fn run_server_link(
    args: &Args,
    down_tx: broadcast::Sender<DownEvent>,
    up_rx: Arc<Mutex<mpsc::Receiver<UpEvent>>>,
) -> Result<()> {
    // Helpvolle hint als wss:// gebruikt wordt en TLS niet gecompileerd is
    if args.server.starts_with("wss://") && !cfg!(feature = "tls") {
        return Err(anyhow!(
            "Server URL gebruikt wss:// maar deze build heeft geen TLS support. \
             Gebruik ws:// (aanbevolen, tunnel via VPN) of hercompileer met: \
             cargo build --release --features tls"
        ));
    }
    // Verbind met de server. Voor ws:// maken we de TCP-socket zelf zodat
    // we TCP_NODELAY kunnen zetten (lagere latency, geen Nagle). Voor wss://
    // (TLS) gebruiken we de standaard connect_async.
    let ws = if args.server.starts_with("ws://") {
        // Parse host:port uit de ws:// URL.
        let url = url::Url::parse(&args.server)
            .with_context(|| format!("Ongeldige server URL: {}", args.server))?;
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("Geen host in URL: {}", args.server))?;
        let port = url.port().unwrap_or(80);
        let tcp = tokio::net::TcpStream::connect((host, port))
            .await
            .with_context(|| format!("Kon geen TCP-verbinding maken met {}:{}", host, port))?;
        if let Err(e) = tcp.set_nodelay(true) {
            warn!("Kon TCP_NODELAY niet zetten: {}", e);
        }
        // Wrap in MaybeTlsStream::Plain zodat het type gelijk is aan de
        // wss:// tak (connect_async geeft WebSocketStream<MaybeTlsStream<_>>).
        let maybe_tls = tokio_tungstenite::MaybeTlsStream::Plain(tcp);
        let (ws, _) = tokio_tungstenite::client_async(&args.server, maybe_tls)
            .await
            .with_context(|| format!("WebSocket handshake mislukt met {}", args.server))?;
        ws
    } else {
        let (ws, _) = connect_async(&args.server)
            .await
            .with_context(|| format!("Kon geen verbinding maken met {}", args.server))?;
        ws
    };
    let (mut tx, mut rx) = ws.split();
    info!("Verbonden met server");

    // Bouw audio config uit CLI args.
    let audio_config = match args.codec {
        CliCodec::Opus => AudioConfig {
            codec: AudioCodec::Opus,
            sample_rate: args.sample_rate,
            frame_dms: (args.frame_ms * 10.0).round() as u16,
            bitrate: args.bitrate,
            channels: args.channels.into(),
        },
        CliCodec::Flac => AudioConfig::flac(args.sample_rate, args.channels.into()),
        CliCodec::Lossless => AudioConfig::lossless(),
        CliCodec::LosslessInt16 => AudioConfig::lossless_int16(),
    };
    if let Err(e) = audio_config.validate() {
        return Err(anyhow!("Ongeldige audio config: {}", e));
    }

    // Filter cutoffs uit preset.
    let rx_preset: tci_streamer::codec::filter::AudioFilterPreset = args.rx_filter.into();
    let tx_preset: tci_streamer::codec::filter::AudioFilterPreset = args.tx_filter.into();
    let (rx_hp, rx_lp) = rx_preset.cutoffs();
    let (tx_hp, tx_lp) = tx_preset.cutoffs();

    let hello = Hello {
        protocol_version: 1,
        iq_mode: args.iq_mode.into(),
        audio: audio_config,
        spectrum_fps: args.spectrum_fps,
        spectrum_bins: args.spectrum_bins,
        rx_hp_hz: rx_hp as u16,
        rx_lp_hz: rx_lp as u16,
        tx_hp_hz: tx_hp as u16,
        tx_lp_hz: tx_lp as u16,
    };
    let mut hello_buf = vec![FrameType::Hello as u8];
    hello_buf.extend_from_slice(&serde_json::to_vec(&hello)?);
    tx.send(Message::Binary(hello_buf)).await?;

    info!(
        "Audio config: {:?} {}Hz {:.1}ms {}bps {:?}",
        audio_config.codec,
        audio_config.sample_rate,
        audio_config.frame_ms(),
        audio_config.bitrate,
        audio_config.channels,
    );
    info!(
        "Filter cutoffs: RX {}-{} Hz, TX {}-{} Hz",
        rx_hp, rx_lp, tx_hp, tx_lp
    );

    match rx.next().await {
        Some(Ok(Message::Binary(data))) => {
            let (ft, _) = parse_frame(&data)?;
            if ft != FrameType::HelloAck {
                return Err(anyhow!("Verwachtte HelloAck, kreeg {:?}", ft));
            }
        }
        _ => return Err(anyhow!("Geen HelloAck ontvangen")),
    }
    info!("Hello geaccepteerd door server");

    // RX decoder: ontvangt Opus-pakketten van server in codec layout (uit
    // audio_config) en levert TCI-formaat (48kHz stereo float32) aan THRA.
    let mut audio_dec: Option<AudioDecoder> = if audio_config.uses_opus() {
        Some(AudioDecoder::new(
            audio_config.channels.count(),
            audio_config.sample_rate,
            2,      // THRA krijgt stereo
            48_000, // THRA verwacht 48kHz
        )?)
    } else {
        None
    };

    // TX encoder: krijgt TCI-formaat audio van THRA (48kHz stereo float32)
    // en encodeert naar Opus in codec layout om naar server te sturen.
    let mut tx_encoder: Option<AudioEncoder> = if audio_config.uses_opus() {
        Some(AudioEncoder::new(audio_config, 2, 48_000)?)
    } else {
        None
    };

    // FLAC RX decoder en TX encoder (alleen actief bij codec=flac).
    let flac_rx_decoder: Option<FlacDecoder> = if audio_config.uses_flac() {
        Some(FlacDecoder::new(2)) // THRA krijgt stereo
    } else {
        None
    };
    let flac_tx_encoder: Option<FlacEncoder> = if audio_config.uses_flac() {
        Some(FlacEncoder::new(audio_config.channels.count(), 48_000)?)
    } else {
        None
    };

    // Filters draaien op 48kHz (TCI-formaat) — VOORDAT Opus encoding (TX)
    // of NA Opus decoding (RX). Cutoffs komen uit Hello.
    let mut rx_filter = BandpassFilter::new(48_000, rx_hp, rx_lp);
    let mut tx_filter = BandpassFilter::new(48_000, tx_hp, tx_lp);
    if rx_filter.is_active() {
        info!(
            "Client RX filter actief: hp={:.0} Hz, lp={:.0} Hz",
            rx_filter.hp_hz(),
            rx_filter.lp_hz()
        );
    }
    if tx_filter.is_active() {
        info!(
            "Client TX filter actief: hp={:.0} Hz, lp={:.0} Hz",
            tx_filter.hp_hz(),
            tx_filter.lp_hz()
        );
    }

    let mut up_rx_guard = up_rx.lock().await;

    // Helper: stuur gedecodeerde RX audio (48kHz stereo f32) direct als TCI
    // binary frame naar de TCI-client. (De jitter-buffer uit v0.1.17 is
    // verwijderd: die produceerde zelf een 23.4 Hz ratel doordat zijn
    // vaste pull-timer niet synchroon liep met de audio-instroom. Met het
    // MTU-probleem opgelost is directe doorgifte schoon.)
    macro_rules! route_rx_audio {
        ($pcm:expr, $sample_rate:expr) => {{
            let bin = build_tci_binary(TciStreamKind::RxAudio, 0, $sample_rate, &$pcm);
            let _ = down_tx.send(DownEvent::Binary(Arc::new(bin)));
        }};
    }

    loop {
        tokio::select! {
            srv = rx.next() => {
                let msg = match srv {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => return Err(anyhow!("WS error: {}", e)),
                    None => return Err(anyhow!("Server sloot connectie")),
                };
                match msg {
                    Message::Binary(data) => {
                        let (ft, payload) = parse_frame(&data)?;
                        match ft {
                            FrameType::TciText => {
                                let text = String::from_utf8_lossy(payload).to_string();
                                let _ = down_tx.send(DownEvent::Text(text));
                            }
                            FrameType::OpusRxAudio => {
                                if payload.len() < 8 { continue; }
                                // Belangrijk: sample_rate in het Opus frame
                                // is de CODEC rate (audio_config.sample_rate).
                                // De decoder converteert intern naar 48kHz
                                // stereo voor THRA, dus we sturen het TCI
                                // frame met 48000.
                                let _codec_rate = LittleEndian::read_u32(&payload[0..4]);
                                let opus_bytes = &payload[8..];
                                let Some(dec) = audio_dec.as_mut() else {
                                    warn!("OpusRxAudio ontvangen maar codec=lossless. Frame genegeerd.");
                                    continue;
                                };
                                if let Ok(mut pcm) = dec.decode(opus_bytes) {
                                    if rx_filter.is_active() {
                                        rx_filter.set_sample_rate(48_000);
                                        rx_filter.process_stereo(&mut pcm);
                                    }
                                    route_rx_audio!(pcm, 48_000);
                                }
                            }
                            FrameType::RawRxAudio => {
                                let raw = match tci_streamer::proto::parse_raw_audio(payload) {
                                    Ok(r) => r,
                                    Err(_) => continue,
                                };
                                let mut pcm: Vec<f32> = match raw.format {
                                    tci_streamer::proto::RAW_AUDIO_FORMAT_FLOAT32 => {
                                        let n = raw.sample_bytes.len() / 4;
                                        let mut v = Vec::with_capacity(n);
                                        for i in 0..n {
                                            v.push(LittleEndian::read_f32(
                                                &raw.sample_bytes[i * 4..i * 4 + 4],
                                            ));
                                        }
                                        v
                                    }
                                    tci_streamer::proto::RAW_AUDIO_FORMAT_INT16 => {
                                        let n = raw.sample_bytes.len() / 2;
                                        let mut v = Vec::with_capacity(n);
                                        for i in 0..n {
                                            let s = LittleEndian::read_i16(
                                                &raw.sample_bytes[i * 2..i * 2 + 2],
                                            );
                                            v.push(s as f32 / 32767.0);
                                        }
                                        v
                                    }
                                    _ => continue,
                                };
                                if rx_filter.is_active() {
                                    rx_filter.set_sample_rate(raw.sample_rate);
                                    rx_filter.process_stereo(&mut pcm);
                                }
                                route_rx_audio!(pcm, raw.sample_rate);
                            }
                            FrameType::FlacRxAudio => {
                                if payload.len() < 8 { continue; }
                                let flac_bytes = &payload[8..];
                                let Some(dec) = flac_rx_decoder.as_ref() else {
                                    warn!("FlacRxAudio ontvangen maar codec != flac. Frame genegeerd.");
                                    continue;
                                };
                                match dec.decode_chunk(flac_bytes) {
                                    Ok((mut pcm, _sr)) => {
                                        if rx_filter.is_active() {
                                            rx_filter.set_sample_rate(48_000);
                                            rx_filter.process_stereo(&mut pcm);
                                        }
                                        route_rx_audio!(pcm, 48_000);
                                    }
                                    Err(e) => {
                                        warn!("FLAC decode fout: {:#}", e);
                                    }
                                }
                            }
                            FrameType::SpectrumU8 => {
                                if payload.len() < 12 { continue; }
                                let span = LittleEndian::read_u32(&payload[4..8]);
                                let bin_count = LittleEndian::read_u16(&payload[8..10]) as usize;
                                let db_min = payload[10] as i8;
                                let db_max = payload[11] as i8;
                                if payload.len() < 12 + bin_count { continue; }
                                let bins = &payload[12..12 + bin_count];
                                let iq = synth_iq_from_spectrum(bins, db_min, db_max, bin_count.max(1024));
                                let bin = build_tci_binary(TciStreamKind::Iq, 0, span, &iq);
                                let _ = down_tx.send(DownEvent::Binary(Arc::new(bin)));
                            }
                            FrameType::IqInt16 => {
                                if payload.len() < 8 { continue; }
                                let sample_rate = LittleEndian::read_u32(&payload[0..4]);
                                let count = LittleEndian::read_u32(&payload[4..8]) as usize;
                                let bytes_needed = count * 2 * 2;
                                if payload.len() < 8 + bytes_needed { continue; }
                                let mut i16s = Vec::with_capacity(count * 2);
                                let mut i = 8;
                                while i + 2 <= 8 + bytes_needed {
                                    i16s.push(LittleEndian::read_i16(&payload[i..i + 2]));
                                    i += 2;
                                }
                                let f32s = i16_to_float_iq(&i16s);
                                let bin = build_tci_binary(TciStreamKind::Iq, 0, sample_rate, &f32s);
                                let _ = down_tx.send(DownEvent::Binary(Arc::new(bin)));
                            }
                            FrameType::RawTciBinary => {
                                // Diagnostic passthrough: stuur het ruwe TCI binary
                                // frame 1:1 door naar de lokale TCI clients.
                                let _ = down_tx.send(DownEvent::Binary(Arc::new(payload.to_vec())));
                            }
                            FrameType::Heartbeat => {
                                let _ = tx.send(Message::Binary(vec![FrameType::Heartbeat as u8])).await;
                            }
                            _ => {}
                        }
                    }
                    Message::Close(_) => return Err(anyhow!("Server sloot connectie")),
                    _ => {}
                }
            }

            up = up_rx_guard.recv() => {
                match up {
                    Some(UpEvent::Text(t)) => {
                        let mut buf = vec![FrameType::TciText as u8];
                        buf.extend_from_slice(t.as_bytes());
                        tx.send(Message::Binary(buf)).await?;
                    }
                    Some(UpEvent::TxAudioBinary(data)) => {
                        // THRA stuurt TCI binary frame met TX audio (48kHz
                        // stereo float32). Parse, filter, en stuur naar
                        // server als Opus of Raw audio.
                        let (kind, sample_rate, sample_count, _off) =
                            match tci_streamer::proto::tci::classify_tci_binary(&data) {
                                Some(x) => x,
                                None => continue,
                            };
                        if kind != TciStreamKind::TxAudioStream {
                            // Geen TX audio (bv. mic data of iets anders)
                            continue;
                        }
                        let mut samples = tci_streamer::proto::tci::read_f32_samples(
                            &data,
                            sample_count,
                        );

                        if tx_filter.is_active() {
                            tx_filter.set_sample_rate(sample_rate);
                            tx_filter.process_stereo(&mut samples);
                        }

                        match audio_config.codec {
                            AudioCodec::Opus => {
                                if let Some(enc) = tx_encoder.as_mut() {
                                    for opus in enc.push(&samples)? {
                                        let frame = tci_streamer::proto::build_opus_audio(
                                            true,
                                            enc.sample_rate(),
                                            enc.channels(),
                                            &opus,
                                        );
                                        tx.send(Message::Binary(frame)).await?;
                                    }
                                }
                            }
                            AudioCodec::Flac => {
                                if let Some(enc) = flac_tx_encoder.as_ref() {
                                    // Mono downmix indien geconfigureerd.
                                    let to_encode: Vec<f32> = if audio_config.channels
                                        == AudioChannels::Mono
                                    {
                                        let n = samples.len() / 2;
                                        let mut mono = Vec::with_capacity(n);
                                        for i in 0..n {
                                            mono.push(
                                                (samples[i * 2] + samples[i * 2 + 1]) * 0.5,
                                            );
                                        }
                                        mono
                                    } else {
                                        samples.clone()
                                    };
                                    let flac_bytes = enc.encode_chunk(&to_encode)?;
                                    let frame = tci_streamer::proto::build_flac_audio(
                                        true,
                                        sample_rate,
                                        audio_config.channels.count(),
                                        &flac_bytes,
                                    );
                                    tx.send(Message::Binary(frame)).await?;
                                }
                            }
                            AudioCodec::Lossless => {
                                let samples_per_channel =
                                    (samples.len() / 2).min(u16::MAX as usize) as u16;
                                let mut bytes = Vec::with_capacity(samples.len() * 4);
                                for &s in &samples {
                                    let mut tmp = [0u8; 4];
                                    LittleEndian::write_f32(&mut tmp, s);
                                    bytes.extend_from_slice(&tmp);
                                }
                                let frame = tci_streamer::proto::build_raw_audio(
                                    true,
                                    sample_rate,
                                    2,
                                    tci_streamer::proto::RAW_AUDIO_FORMAT_FLOAT32,
                                    samples_per_channel,
                                    &bytes,
                                );
                                tx.send(Message::Binary(frame)).await?;
                            }
                            AudioCodec::LosslessInt16 => {
                                let samples_per_channel =
                                    (samples.len() / 2).min(u16::MAX as usize) as u16;
                                let mut bytes = Vec::with_capacity(samples.len() * 2);
                                for &s in &samples {
                                    let clamped = s.clamp(-1.0, 1.0);
                                    let i = (clamped * 32767.0) as i16;
                                    let mut tmp = [0u8; 2];
                                    LittleEndian::write_i16(&mut tmp, i);
                                    bytes.extend_from_slice(&tmp);
                                }
                                let frame = tci_streamer::proto::build_raw_audio(
                                    true,
                                    sample_rate,
                                    2,
                                    tci_streamer::proto::RAW_AUDIO_FORMAT_INT16,
                                    samples_per_channel,
                                    &bytes,
                                );
                                tx.send(Message::Binary(frame)).await?;
                            }
                        }
                    }
                    None => return Err(anyhow!("Up-channel gesloten")),
                }
            }
        }
    }
}

async fn handle_local_tci_client(
    stream: tokio::net::TcpStream,
    mut down_rx: broadcast::Receiver<DownEvent>,
    up_tx: mpsc::Sender<UpEvent>,
) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut tx, mut rx) = ws.split();

    loop {
        tokio::select! {
            evt = down_rx.recv() => {
                match evt {
                    Ok(DownEvent::Text(t)) => {
                        tx.send(Message::Text(t)).await?;
                    }
                    Ok(DownEvent::Binary(b)) => {
                        tx.send(Message::Binary((*b).clone())).await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Lokale client mist {} frames (te traag)", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
            cl = rx.next() => {
                match cl {
                    Some(Ok(Message::Text(t))) => {
                        let _ = up_tx.send(UpEvent::Text(t)).await;
                    }
                    Some(Ok(Message::Binary(b))) => {
                        let _ = up_tx.send(UpEvent::TxAudioBinary(b)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}
