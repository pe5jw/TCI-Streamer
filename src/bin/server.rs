//! tci-streamer-server
//!
//! Draait op de PC waar Thetis/Zeus draait. Verbindt met de upstream TCI-server
//! (default ws://127.0.0.1:40001) en biedt zelf een gecomprimeerde WebSocket
//! aan voor remote clients.
//!
//! Eén client per server-instance (start meerdere instances voor meerdere
//! clients of meerdere upstreams).

use anyhow::{anyhow, Context, Result};
use byteorder::{ByteOrder, LittleEndian};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tci_streamer::codec::audio::{AudioDecoder, AudioEncoder};
use tci_streamer::codec::filter::BandpassFilter;
use tci_streamer::codec::flac::{FlacDecoder, FlacEncoder};
use tci_streamer::codec::iq::{IqDecimator, SpectrumProcessor};
use tci_streamer::proto::tci::{
    build_tci_binary, classify_tci_binary, read_f32_samples, TciStreamKind,
};
use tci_streamer::proto::{
    build_flac_audio, build_iq_int16, build_opus_audio, build_raw_audio, build_spectrum_u8,
    build_tci_text, parse_frame, AudioCodec, FrameType, Hello, IqMode, RAW_AUDIO_FORMAT_FLOAT32,
    RAW_AUDIO_FORMAT_INT16,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

#[derive(Parser, Debug, Clone)]
#[command(name = "tci-streamer-server")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "TCI proxy server with bandwidth compression")]
struct Args {
    /// Upstream TCI WebSocket URL (Thetis/Zeus)
    #[arg(long, default_value = "ws://127.0.0.1:40001")]
    upstream: String,

    /// Listen address voor remote clients
    #[arg(long, default_value = "0.0.0.0:9001")]
    listen: SocketAddr,

    /// Decimated IQ target sample rate (alleen bij IqMode::DecimatedIq)
    #[arg(long, default_value = "48000")]
    iq_target_rate: u32,

    /// Spectrum FFT size (bij IqMode::Spectrum)
    #[arg(long, default_value = "4096")]
    fft_size: usize,

    /// IQ-swap: corrigeert het verschil in IQ-conventie tussen een Zeus- en
    /// een Thetis-server (de waterfall is anders gespiegeld).
    ///   - none: geen wijziging
    ///   - swap: verwissel I en Q (spiegelt spectrum)
    ///   - conj: keer Q-teken om (spiegelt rond DC)
    /// Probeer 'swap' als de waterfall gespiegeld is; 'conj' als alternatief.
    #[arg(long, value_enum, default_value = "none")]
    iq_swap: CliIqSwap,

    /// Diagnostic passthrough: stuur TCI binary frames 1:1 door zonder
    /// codec of FFT. Negeert hello.iq_mode en hello.audio.
    /// Bandbreedte is niet gereduceerd; alleen voor debugging.
    #[arg(long)]
    passthrough: bool,

    /// Log de eerste N binary frames met hex-dump van header voor debugging.
    #[arg(long, default_value = "0")]
    log_first_frames: u32,

    /// Log alle TCI text commando's (in/uit). Helpt bij debuggen van de
    /// stream-init flow (welke audio_start/iq_start/etc. commando's gaan
    /// over en wanneer).
    #[arg(long)]
    log_tci_commands: bool,

    /// Stuur een init-sequentie naar de upstream (Thetis/Zeus) zodra de
    /// verbinding staat: audio_start, iq_start, start. Standaard UIT —
    /// normaal regelt de TCI-client (THRA/N1MM/JTDX die via de tci-streamer
    /// client verbindt) deze commando's. Zet aan om te testen zonder THRA,
    /// bv. met alleen tci-viewer als afnemer.
    #[arg(long)]
    auto_start: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum CliIqSwap {
    None,
    Swap,
    Conj,
}

impl From<CliIqSwap> for tci_streamer::codec::iq::IqSwap {
    fn from(v: CliIqSwap) -> Self {
        match v {
            CliIqSwap::None => Self::None,
            CliIqSwap::Swap => Self::SwapIQ,
            CliIqSwap::Conj => Self::ConjQ,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tci_streamer=info".parse()?)
                .add_directive("tci_streamer_server=info".parse()?),
        )
        .init();

    let args = Args::parse();
    info!("TCI Streamer Server v{} gestart", env!("CARGO_PKG_VERSION"));
    info!("  Upstream: {}", args.upstream);
    info!("  Listen:   {}", args.listen);

    let listener = TcpListener::bind(args.listen)
        .await
        .with_context(|| format!("Kon niet luisteren op {}", args.listen))?;

    loop {
        let (stream, peer) = listener.accept().await?;
        // TCP_NODELAY: kleine frames meteen versturen (lagere latency,
        // geen Nagle-buffering).
        if let Err(e) = stream.set_nodelay(true) {
            warn!("Kon TCP_NODELAY niet zetten op {}: {}", peer, e);
        }
        info!("Nieuwe client van {}", peer);
        let args = args.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, args).await {
                warn!("Client {} sessie eindigde: {:#}", peer, e);
            }
        });
    }
}

async fn handle_client(stream: tokio::net::TcpStream, args: Args) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .context("WebSocket handshake failed")?;
    let (mut client_tx, mut client_rx) = ws.split();

    // Wacht op Hello van client
    let hello = match client_rx.next().await {
        Some(Ok(Message::Binary(data))) => {
            let (ft, payload) = parse_frame(&data)?;
            if ft != FrameType::Hello {
                return Err(anyhow!("Verwachtte Hello, kreeg {:?}", ft));
            }
            serde_json::from_slice::<Hello>(payload).context("Hello JSON parse fout")?
        }
        _ => return Err(anyhow!("Geen Hello ontvangen")),
    };
    info!(
        "Client wil: iq_mode={:?}, audio={:?} {}Hz {:.1}ms {}bps {:?}, fps={}, bins={}",
        hello.iq_mode,
        hello.audio.codec,
        hello.audio.sample_rate,
        hello.audio.frame_ms(),
        hello.audio.bitrate,
        hello.audio.channels,
        hello.spectrum_fps,
        hello.spectrum_bins
    );

    // ACK terug
    let mut ack = vec![FrameType::HelloAck as u8];
    ack.extend_from_slice(&serde_json::to_vec(&hello)?);
    client_tx.send(Message::Binary(ack)).await?;

    // Connect upstream
    let (upstream_ws, _) = connect_async(&args.upstream)
        .await
        .with_context(|| format!("Kon niet verbinden met upstream {}", args.upstream))?;
    let (upstream_tx, mut upstream_rx) = upstream_ws.split();
    info!("Upstream verbonden");

    let upstream_tx = Arc::new(Mutex::new(upstream_tx));

    // Auto-start: stuur de init-sequentie naar upstream (Thetis/Zeus) zodat
    // de streams beginnen, zelfs als er geen TCI-client (THRA) verbonden is.
    // Handig voor testen met alleen tci-viewer.
    if args.auto_start {
        let init = [
            "audio_samplerate:48000;",
            "iq_samplerate:48000;",
            "rx_enable:0,true;",
            "audio_start:0;",
            "iq_start:0;",
            "start;",
        ];
        let mut up = upstream_tx.lock().await;
        for cmd in init {
            if let Err(e) = up.send(Message::Text(cmd.to_string())).await {
                warn!("Auto-start commando '{}' faalde: {}", cmd, e);
                break;
            }
            if args.log_tci_commands {
                info!("→ upstream (auto-start): {}", cmd);
            }
        }
        info!("Auto-start init-sequentie verstuurd naar upstream");
    }

    // State voor codec
    let mut audio_encoder: Option<AudioEncoder> = None;
    let mut tx_decoder: Option<AudioDecoder> = None;
    let mut flac_encoder: Option<FlacEncoder> = None;
    let mut flac_tx_decoder: Option<FlacDecoder> = None;
    let mut spectrum: Option<SpectrumProcessor> = None;
    let mut iq_decim: Option<IqDecimator> = None;
    let mut iq_center_hz: u32 = 0;
    let mut frames_logged: u32 = 0;
    let mut first_audio_log = true;

    // Audio bandpass filters (server-side). Cutoffs komen uit Hello
    // (client beslist) zodat server en client dezelfde filter-config hebben.
    let mut rx_filter = BandpassFilter::new(48_000, hello.rx_hp_hz as f32, hello.rx_lp_hz as f32);
    let mut tx_filter = BandpassFilter::new(48_000, hello.tx_hp_hz as f32, hello.tx_lp_hz as f32);
    if rx_filter.is_active() {
        info!(
            "Server RX filter actief: hp={:.0} Hz, lp={:.0} Hz",
            rx_filter.hp_hz(),
            rx_filter.lp_hz()
        );
    }
    if tx_filter.is_active() {
        info!(
            "Server TX filter actief: hp={:.0} Hz, lp={:.0} Hz",
            tx_filter.hp_hz(),
            tx_filter.lp_hz()
        );
    }

    // Stream type counters voor diagnostiek
    let mut stream_count_iq: u32 = 0;
    let mut stream_count_rx_audio: u32 = 0;
    let mut stream_count_tx_audio: u32 = 0;
    let mut stream_count_other: u32 = 0;
    let mut last_stat_log = std::time::Instant::now();

    // Channels voor cross-task communicatie
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(256);

    // Task: forward gecomprimeerde data naar de client.
    // We gebruiken een AbortOnDrop wrapper zodat de task eindigt zodra deze
    // functie returnt (via een error of normaal).
    let send_task = tokio::spawn(async move {
        while let Some(buf) = out_rx.recv().await {
            if client_tx.send(Message::Binary(buf)).await.is_err() {
                break;
            }
        }
    });
    let _send_task_guard = AbortOnDrop(send_task);

    // Hoofdloop: switch tussen upstream en client
    loop {
        tokio::select! {
            // From upstream (Thetis) -> compress -> client
            up = upstream_rx.next() => {
                let msg = match up {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => return Err(anyhow!("Upstream WS error: {}", e)),
                    None => return Err(anyhow!("Upstream WS gesloten")),
                };
                match msg {
                    Message::Text(text) => {
                        // Log elk TCI command zodat we de stream-init flow zien
                        if args.log_tci_commands {
                            // Splits op ; en log elk niet-leeg commando
                            for cmd in text.split(';').map(str::trim).filter(|s| !s.is_empty()) {
                                info!("TCI <- {}", cmd);
                            }
                        }
                        // Parse interessante TCI commands
                        if let Some(rate) = parse_kv_u32(&text, "iq_sample_rate:") {
                            iq_decim = Some(IqDecimator::new(rate, args.iq_target_rate));
                        }
                        if let Some(hz) = parse_vfo(&text) {
                            iq_center_hz = hz;
                        }
                        // Forward 1:1
                        let frame = build_tci_text(&text);
                        let _ = out_tx.send(frame).await;
                    }
                    Message::Binary(data) => {
                        // Stream type counters: per 5 seconden hoeveel frames van elke kind
                        let kind_for_count = match classify_tci_binary(&data) {
                            Some((k, _, _, _)) => k,
                            None => TciStreamKind::Unknown,
                        };
                        match kind_for_count {
                            TciStreamKind::Iq => stream_count_iq += 1,
                            TciStreamKind::RxAudio => stream_count_rx_audio += 1,
                            TciStreamKind::TxAudioStream => stream_count_tx_audio += 1,
                            _ => stream_count_other += 1,
                        }
                        if last_stat_log.elapsed() >= std::time::Duration::from_secs(5) {
                            info!(
                                "Stream stats (laatste 5s): IQ={} RxAudio={} TxAudio={} Anders={}",
                                stream_count_iq, stream_count_rx_audio,
                                stream_count_tx_audio, stream_count_other
                            );
                            stream_count_iq = 0;
                            stream_count_rx_audio = 0;
                            stream_count_tx_audio = 0;
                            stream_count_other = 0;
                            last_stat_log = std::time::Instant::now();
                        }

                        // Diagnostic logging: dump headers van de eerste paar binary frames
                        if frames_logged < args.log_first_frames {
                            log_binary_frame_header(&data, frames_logged);
                            frames_logged += 1;
                        }

                        // Passthrough mode: stuur 1:1 door zonder enige bewerking
                        if args.passthrough {
                            let mut frame = Vec::with_capacity(1 + data.len());
                            frame.push(FrameType::RawTciBinary as u8);
                            frame.extend_from_slice(&data);
                            let _ = out_tx.send(frame).await;
                            continue;
                        }

                        let (kind, sample_rate, sample_count, _off) = match classify_tci_binary(&data) {
                            Some(x) => x,
                            None => continue,
                        };
                        let mut samples = read_f32_samples(&data, sample_count);
                        match kind {
                            TciStreamKind::RxAudio => {
                                // Update filter sample rate als nodig en pas
                                // server-side RX bandpass toe (in-place).
                                if rx_filter.is_active() {
                                    rx_filter.set_sample_rate(sample_rate);
                                    rx_filter.process_stereo(&mut samples);
                                }
                                match hello.audio.codec {
                                    AudioCodec::Lossless | AudioCodec::LosslessInt16 => {
                                        if first_audio_log {
                                            info!(
                                                "Eerste RX audio frame (lossless {:?}): rate={} Hz, samples={}",
                                                hello.audio.codec, sample_rate, samples.len()
                                            );
                                            first_audio_log = false;
                                        }
                                        let samples_per_channel =
                                            (samples.len() / 2).min(u16::MAX as usize) as u16;
                                        let frame = if hello.audio.codec == AudioCodec::Lossless {
                                            let mut bytes = Vec::with_capacity(samples.len() * 4);
                                            for &s in &samples {
                                                let mut tmp = [0u8; 4];
                                                LittleEndian::write_f32(&mut tmp, s);
                                                bytes.extend_from_slice(&tmp);
                                            }
                                            build_raw_audio(
                                                false,
                                                sample_rate,
                                                2,
                                                RAW_AUDIO_FORMAT_FLOAT32,
                                                samples_per_channel,
                                                &bytes,
                                            )
                                        } else {
                                            let mut bytes = Vec::with_capacity(samples.len() * 2);
                                            for &s in &samples {
                                                let clamped = s.clamp(-1.0, 1.0);
                                                let i = (clamped * 32767.0) as i16;
                                                let mut tmp = [0u8; 2];
                                                LittleEndian::write_i16(&mut tmp, i);
                                                bytes.extend_from_slice(&tmp);
                                            }
                                            build_raw_audio(
                                                false,
                                                sample_rate,
                                                2,
                                                RAW_AUDIO_FORMAT_INT16,
                                                samples_per_channel,
                                                &bytes,
                                            )
                                        };
                                        let _ = out_tx.send(frame).await;
                                    }
                                    AudioCodec::Opus => {
                                        if audio_encoder.is_none() {
                                            info!(
                                                "Eerste RX audio frame (Opus {}Hz {:.1}ms {}bps {:?}): input rate={} Hz, samples={}",
                                                hello.audio.sample_rate,
                                                hello.audio.frame_ms(),
                                                hello.audio.bitrate,
                                                hello.audio.channels,
                                                sample_rate,
                                                samples.len()
                                            );
                                            audio_encoder = Some(AudioEncoder::new(
                                                hello.audio,
                                                2,           // Thetis stuurt stereo
                                                sample_rate, // Thetis sample rate
                                            )?);
                                        }
                                        if let Some(enc) = audio_encoder.as_mut() {
                                            for opus in enc.push(&samples)? {
                                                let frame = build_opus_audio(
                                                    false,
                                                    enc.sample_rate(),
                                                    enc.channels(),
                                                    &opus,
                                                );
                                                let _ = out_tx.send(frame).await;
                                            }
                                        }
                                    }
                                    AudioCodec::Flac => {
                                        if flac_encoder.is_none() {
                                            info!(
                                                "Eerste RX audio frame (FLAC {:?}): rate={} Hz, samples={}",
                                                hello.audio.channels, sample_rate, samples.len()
                                            );
                                            flac_encoder = Some(FlacEncoder::new(
                                                hello.audio.channels.count(),
                                                sample_rate,
                                            )?);
                                        }
                                        if let Some(enc) = flac_encoder.as_ref() {
                                            // Mono downmix indien nodig (Thetis
                                            // stuurt stereo, FLAC config kan mono zijn).
                                            let to_encode: Vec<f32> = if hello.audio.channels
                                                == tci_streamer::proto::AudioChannels::Mono
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
                                            let frame = build_flac_audio(
                                                false,
                                                sample_rate,
                                                hello.audio.channels.count(),
                                                &flac_bytes,
                                            );
                                            let _ = out_tx.send(frame).await;
                                        }
                                    }
                                }
                            }
                            TciStreamKind::Iq => {
                                // Pas IQ-swap toe (Zeus/Thetis conventie-verschil)
                                // vóór FFT/decimatie, zodat de waterfall correct
                                // georiënteerd is.
                                let iq_swap: tci_streamer::codec::iq::IqSwap =
                                    args.iq_swap.into();
                                iq_swap.apply(&mut samples);
                                match hello.iq_mode {
                                    IqMode::Spectrum => {
                                        if spectrum.is_none() {
                                            spectrum = Some(SpectrumProcessor::new(
                                                args.fft_size,
                                                hello.spectrum_bins as usize,
                                                hello.spectrum_fps,
                                            ));
                                        }
                                        if let Some(s) = spectrum.as_mut() {
                                            if let Some(frm) = s.push(&samples) {
                                                let frame = build_spectrum_u8(
                                                    iq_center_hz,
                                                    sample_rate,
                                                    frm.db_min,
                                                    frm.db_max,
                                                    &frm.bins_u8,
                                                );
                                                let _ = out_tx.send(frame).await;
                                            }
                                        }
                                    }
                                    IqMode::DecimatedIq => {
                                        if iq_decim.is_none() {
                                            iq_decim = Some(IqDecimator::new(sample_rate, args.iq_target_rate));
                                        }
                                        if let Some(d) = iq_decim.as_mut() {
                                            let i16s = d.push(&samples);
                                            if !i16s.is_empty() {
                                                let frame = build_iq_int16(d.target_rate(), &i16s);
                                                let _ = out_tx.send(frame).await;
                                            }
                                        }
                                    }
                                    IqMode::Disabled => {} // drop
                                }
                            }
                            _ => {} // chrono/tx-stream — voor nu negeren
                        }
                    }
                    Message::Close(_) => return Err(anyhow!("Upstream sloot connectie")),
                    _ => {}
                }
            }

            // From client -> decompress -> upstream
            cl = client_rx.next() => {
                let msg = match cl {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => return Err(anyhow!("Client WS error: {}", e)),
                    None => return Ok(()),
                };
                match msg {
                    Message::Binary(data) => {
                        let (ft, payload) = parse_frame(&data)?;
                        match ft {
                            FrameType::TciText => {
                                let text = std::str::from_utf8(payload)?;
                                upstream_tx.lock().await.send(Message::Text(text.to_string())).await?;
                            }
                            FrameType::OpusTxAudio => {
                                if payload.len() < 8 {
                                    continue;
                                }
                                let _codec_rate = LittleEndian::read_u32(&payload[0..4]);
                                let _codec_channels = payload[4];
                                let opus_bytes = &payload[8..];
                                if tx_decoder.is_none() {
                                    // De codec config zit in `hello.audio`.
                                    // Output is 48kHz stereo (wat Thetis verwacht).
                                    tx_decoder = Some(AudioDecoder::new(
                                        hello.audio.channels.count(),
                                        hello.audio.sample_rate,
                                        2,
                                        48_000,
                                    )?);
                                }
                                if let Some(dec) = tx_decoder.as_mut() {
                                    let mut pcm = dec.decode(opus_bytes)?;
                                    if tx_filter.is_active() {
                                        tx_filter.set_sample_rate(48_000);
                                        tx_filter.process_stereo(&mut pcm);
                                    }
                                    let bin = build_tci_binary(
                                        TciStreamKind::TxAudioStream,
                                        0,
                                        48_000,
                                        &pcm,
                                    );
                                    upstream_tx.lock().await.send(Message::Binary(bin)).await?;
                                }
                            }
                            FrameType::RawTxAudio => {
                                let raw = match tci_streamer::proto::parse_raw_audio(payload) {
                                    Ok(r) => r,
                                    Err(_) => continue,
                                };
                                // Reconstrueer float32 PCM uit het lossless frame
                                let mut pcm: Vec<f32> = match raw.format {
                                    RAW_AUDIO_FORMAT_FLOAT32 => {
                                        let n = raw.sample_bytes.len() / 4;
                                        let mut v = Vec::with_capacity(n);
                                        for i in 0..n {
                                            v.push(LittleEndian::read_f32(
                                                &raw.sample_bytes[i * 4..i * 4 + 4],
                                            ));
                                        }
                                        v
                                    }
                                    RAW_AUDIO_FORMAT_INT16 => {
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
                                if tx_filter.is_active() {
                                    tx_filter.set_sample_rate(raw.sample_rate);
                                    tx_filter.process_stereo(&mut pcm);
                                }
                                let bin = build_tci_binary(
                                    TciStreamKind::TxAudioStream,
                                    0,
                                    raw.sample_rate,
                                    &pcm,
                                );
                                upstream_tx.lock().await.send(Message::Binary(bin)).await?;
                            }
                            FrameType::FlacTxAudio => {
                                if payload.len() < 8 {
                                    continue;
                                }
                                let sample_rate = LittleEndian::read_u32(&payload[0..4]);
                                let flac_bytes = &payload[8..];
                                if flac_tx_decoder.is_none() {
                                    // Output 48kHz stereo voor Thetis.
                                    flac_tx_decoder = Some(FlacDecoder::new(2));
                                }
                                if let Some(dec) = flac_tx_decoder.as_ref() {
                                    let (mut pcm, _sr) = dec.decode_chunk(flac_bytes)?;
                                    if tx_filter.is_active() {
                                        tx_filter.set_sample_rate(48_000);
                                        tx_filter.process_stereo(&mut pcm);
                                    }
                                    let bin = build_tci_binary(
                                        TciStreamKind::TxAudioStream,
                                        0,
                                        sample_rate,
                                        &pcm,
                                    );
                                    upstream_tx.lock().await.send(Message::Binary(bin)).await?;
                                }
                            }
                            FrameType::Heartbeat => {
                                // Echo terug
                                let _ = out_tx.send(vec![FrameType::Heartbeat as u8]).await;
                            }
                            _ => {}
                        }
                    }
                    Message::Close(_) => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

fn parse_kv_u32(text: &str, key: &str) -> Option<u32> {
    text.split(';').find_map(|p| {
        let p = p.trim();
        p.strip_prefix(key)
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.parse().ok())
    })
}

fn parse_vfo(text: &str) -> Option<u32> {
    // Format: vfo:<rx>,<channel>,<hz>;
    text.split(';').find_map(|p| {
        let p = p.trim().strip_prefix("vfo:")?;
        let parts: Vec<&str> = p.split(',').collect();
        if parts.len() >= 3 {
            parts[2].parse().ok()
        } else {
            None
        }
    })
}

/// Helper die een spawned task aborts als het scope verlaat.
struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Diagnostic helper: dump de eerste 64 bytes van een binary frame als
/// hex en interpreteer het volgens de huidige TCI binary header layout.
/// Helpt bij debuggen van header-offset issues.
fn log_binary_frame_header(data: &[u8], frame_idx: u32) {
    let n = data.len().min(64);
    let hex: String = data[..n]
        .chunks(4)
        .map(|c| {
            c.iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join("")
        })
        .collect::<Vec<_>>()
        .join(" ");
    info!(
        "FRAME #{} totale lengte={} bytes, eerste {} bytes (per u32 LE):\n  {}",
        frame_idx,
        data.len(),
        n,
        hex
    );
    if data.len() >= 32 {
        let receiver = LittleEndian::read_u32(&data[0..4]);
        let sample_rate = LittleEndian::read_u32(&data[4..8]);
        let format = LittleEndian::read_u32(&data[8..12]);
        let codec = LittleEndian::read_u32(&data[12..16]);
        let crc = LittleEndian::read_u32(&data[16..20]);
        let length = LittleEndian::read_u32(&data[20..24]);
        let stream_type = LittleEndian::read_u32(&data[24..28]);
        let payload_bytes = data.len().saturating_sub(32);
        let payload_floats = payload_bytes / 4;
        info!(
            "  geparsed: receiver={} rate={} format={} codec={} crc={} length={} type={} | payload={}B={}floats={}stereo-samples",
            receiver, sample_rate, format, codec, crc, length, stream_type,
            payload_bytes, payload_floats, payload_floats / 2
        );
    }
}
