//! fake-tci-server — een test-tool die een Thetis/Zeus TCI-server emuleert.
//!
//! Doel: tci-streamer (de echte server, in upstream-rol) hierop laten
//! aansluiten zonder dat je Thetis nodig hebt. Deze fake server:
//!   - Luistert als WebSocket-server (default 127.0.0.1:40001)
//!   - Stuurt periodiek TCI text-commando's (vfo, dds) zoals Thetis doet
//!   - Stuurt RX-audio binary frames: een 1 kHz testtoon (48 kHz stereo)
//!   - Stuurt IQ binary frames: ruis + enkele draaggolf-pieken, zodat het
//!     spectrum/de waterfall herkenbaar is en je IQ-swap kunt testen
//!   - Beantwoordt inkomende TCI text-commando's met een echo/ack
//!
//! Gebruik:
//!   fake-tci-server --listen 127.0.0.1:40001
//!   fake-tci-server --listen 127.0.0.1:40001 --tone 1000 --iq-rate 192000
//!
//! Dan wijs je de echte tci-streamer-server upstream hiernaartoe:
//!   tci-streamer-server --upstream ws://127.0.0.1:40001 --listen 0.0.0.0:9001

use anyhow::{Context, Result};
use byteorder::{ByteOrder, LittleEndian};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::f32::consts::PI;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

#[derive(Parser, Debug, Clone)]
#[command(name = "fake-tci-server",
          version = env!("CARGO_PKG_VERSION"),
          about = "Emuleert een Thetis/Zeus TCI server voor tests")]
struct Args {
    /// Luister-adres (zoals Thetis op 40001).
    #[arg(long, default_value = "127.0.0.1:40001")]
    listen: SocketAddr,

    /// Audio toonfrequentie in Hz (RX testtoon).
    #[arg(long, default_value = "1000")]
    tone: f32,

    /// Audio sample rate.
    #[arg(long, default_value = "48000")]
    audio_rate: u32,

    /// Aantal audio samples per frame (per kanaal). Thetis gebruikt 2048.
    #[arg(long, default_value = "2048")]
    audio_block: usize,

    /// IQ sample rate (Thetis: vaak 48000-192000).
    #[arg(long, default_value = "192000")]
    iq_rate: u32,

    /// Aantal IQ samples per frame (complex paren).
    #[arg(long, default_value = "4096")]
    iq_block: usize,

    /// Stuur géén IQ frames (alleen audio).
    #[arg(long)]
    no_iq: bool,

    /// Stuur géén audio frames (alleen IQ).
    #[arg(long)]
    no_audio: bool,
}

const TCI_HDR: usize = 32;

fn build_tci_binary(stream_type: u32, sample_rate: u32, samples: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(TCI_HDR + samples.len() * 4);
    let mut hdr = [0u8; TCI_HDR];
    LittleEndian::write_u32(&mut hdr[0..4], 0); // receiver 0
    LittleEndian::write_u32(&mut hdr[4..8], sample_rate);
    LittleEndian::write_u32(&mut hdr[8..12], 3); // format float32
    LittleEndian::write_u32(&mut hdr[12..16], 0); // codec PCM
    LittleEndian::write_u32(&mut hdr[16..20], 0); // crc
    LittleEndian::write_u32(&mut hdr[20..24], samples.len() as u32);
    LittleEndian::write_u32(&mut hdr[24..28], stream_type); // 0=IQ 1=RX_AUDIO
    LittleEndian::write_u32(&mut hdr[28..32], 0);
    buf.extend_from_slice(&hdr);
    for &s in samples {
        let mut t = [0u8; 4];
        LittleEndian::write_f32(&mut t, s);
        buf.extend_from_slice(&t);
    }
    buf
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    info!("fake-tci-server v{} gestart", env!("CARGO_PKG_VERSION"));
    info!("  Listen:    {}", args.listen);
    info!("  Audio:     {} Hz toon, {} Hz rate, block {}", args.tone, args.audio_rate, args.audio_block);
    info!("  IQ:        {} Hz rate, block {}", args.iq_rate, args.iq_block);

    let listener = TcpListener::bind(args.listen)
        .await
        .with_context(|| format!("Kon niet luisteren op {}", args.listen))?;

    loop {
        let (stream, peer) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        info!("TCI client verbonden: {}", peer);
        let args = args.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, args).await {
                warn!("Client {} losgekoppeld: {:#}", peer, e);
            }
        });
    }
}

async fn handle(stream: tokio::net::TcpStream, args: Args) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut tx, mut rx) = ws.split();

    // Stuur eerst wat TCI text-commando's zoals Thetis bij verbinden doet.
    let init = [
        "vfo:0,0,14200000;",
        "dds:0,14200000;",
        "if:0,0,0;",
        "modulation:0,usb;",
        "rx_enable:0,true;",
        "audio_samplerate:48000;",
        "iq_samplerate:192000;",
        "start;",
    ];
    for cmd in init {
        tx.send(Message::Text(cmd.to_string())).await?;
    }
    info!("Init TCI commando's verstuurd");

    // Audio/IQ generatie-state
    let mut audio_phase: f32 = 0.0;
    let audio_dt = 2.0 * PI * args.tone / args.audio_rate as f32;

    // IQ: per-toon fase-accumulators voor continue golven over frames heen.
    // Offsets als fractie van de IQ-bandbreedte; asymmetrisch zodat een
    // IQ-swap zichtbaar wordt in de waterfall.
    let iq_tone_fracs = [(0.10f32, 0.5f32), (-0.25, 0.3), (0.33, 0.2)];
    let mut iq_phases = [0.0f32; 3];
    let iq_dts: Vec<f32> = iq_tone_fracs
        .iter()
        .map(|&(frac, _)| 2.0 * PI * frac) // dt per sample = 2pi * (freq/rate) = 2pi * frac
        .collect();
    let mut noise_state: u32 = 12345;

    // Timers
    let audio_period = Duration::from_micros(
        (args.audio_block as u64 * 1_000_000) / args.audio_rate as u64,
    );
    let iq_period = Duration::from_micros(
        (args.iq_block as u64 * 1_000_000) / args.iq_rate as u64,
    );
    let mut audio_timer = tokio::time::interval(audio_period);
    let mut iq_timer = tokio::time::interval(iq_period.max(Duration::from_millis(5)));

    loop {
        tokio::select! {
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        info!("RX commando: {}", t.trim());
                        tx.send(Message::Text(t)).await?;
                    }
                    Some(Ok(Message::Binary(_))) => { /* TX audio van client; negeer */ }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("Client sloot verbinding");
                        return Ok(());
                    }
                    Some(Err(e)) => return Err(e.into()),
                    _ => {}
                }
            }
            _ = audio_timer.tick(), if !args.no_audio => {
                let n = args.audio_block;
                let mut samples = Vec::with_capacity(n * 2);
                for _ in 0..n {
                    let v = audio_phase.sin() * 0.3;
                    audio_phase += audio_dt;
                    if audio_phase > 2.0*PI { audio_phase -= 2.0*PI; }
                    samples.push(v);
                    samples.push(v);
                }
                let frame = build_tci_binary(1, args.audio_rate, &samples);
                if tx.send(Message::Binary(frame)).await.is_err() { return Ok(()); }
            }
            _ = iq_timer.tick(), if !args.no_iq => {
                let n = args.iq_block;
                let mut iq = Vec::with_capacity(n * 2);
                for _ in 0..n {
                    let mut i = 0.0f32;
                    let mut q = 0.0f32;
                    for (k, &(_, amp)) in iq_tone_fracs.iter().enumerate() {
                        i += amp * iq_phases[k].cos();
                        q += amp * iq_phases[k].sin();
                        iq_phases[k] += iq_dts[k];
                        if iq_phases[k] > 2.0*PI { iq_phases[k] -= 2.0*PI; }
                        if iq_phases[k] < -2.0*PI { iq_phases[k] += 2.0*PI; }
                    }
                    i += (next_rand(&mut noise_state) - 0.5) * 0.05;
                    q += (next_rand(&mut noise_state) - 0.5) * 0.05;
                    iq.push(i);
                    iq.push(q);
                }
                let frame = build_tci_binary(0, args.iq_rate, &iq);
                if tx.send(Message::Binary(frame)).await.is_err() { return Ok(()); }
            }
        }
    }
}

// Deterministische pseudo-random in [0,1) (LCG) voor de ruisvloer.
fn next_rand(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    (*state >> 8) as f32 / 16_777_216.0
}
