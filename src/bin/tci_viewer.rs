//! tci-viewer — grafische viewer voor een TCI stream.
//!
//! Toont in een venster:
//!   - IQ-spectrum (live FFT van de IQ-stream, met dB-schaal)
//!   - Waterfall (scrollende kleurgecodeerde 2D weergave over tijd)
//!   - Audio-spectrum (FFT van de RX-audio)
//!   - Audio-niveau (VU)
//!
//! Verbindt als WebSocket-client met een TCI-stream — bv. de lokale listener
//! van de tci-streamer client, of direct met de fake-tci-server.

use anyhow::{Context, Result};
use byteorder::{ByteOrder, LittleEndian};
use clap::Parser;
use eframe::egui;
use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use futures_util::{SinkExt, StreamExt};
use rustfft::{num_complex::Complex32, FftPlanner};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message;

const TCI_HDR: usize = 32;

#[derive(Parser, Debug, Clone)]
#[command(name = "tci-viewer",
          version = env!("CARGO_PKG_VERSION"),
          about = "Grafische viewer voor TCI streams (spectrum + waterfall + audio)")]
struct Args {
    /// TCI WebSocket URL.
    #[arg(long, default_value = "ws://127.0.0.1:40002")]
    connect: String,

    /// FFT-grootte voor het IQ-spectrum.
    #[arg(long, default_value = "1024")]
    iq_fft: usize,

    /// FFT-grootte voor het audio-spectrum.
    #[arg(long, default_value = "1024")]
    audio_fft: usize,

    /// Aantal waterfall-rijen (geschiedenis).
    #[arg(long, default_value = "200")]
    waterfall_rows: usize,

    /// Optionele TCI-commando's om bij verbinden te sturen.
    #[arg(long)]
    cmd: Vec<String>,

    /// Standaard stuurt de viewer een init-sequentie (audio_start, iq_start)
    /// bij verbinden, zodat een Thetis/Zeus TCI server zijn streams begint
    /// te leveren. Zet deze flag aan om dat over te slaan (bv. als je
    /// verbindt met de tci-streamer client of fake server, die uit zichzelf
    /// streamen).
    #[arg(long)]
    no_auto_start: bool,
}

/// Gedeelde state tussen netwerk-task en UI-thread.
struct ViewerState {
    iq_spectrum: Vec<f32>,
    iq_rate: u32,
    audio_spectrum: Vec<f32>,
    audio_rate: u32,
    audio_rms_db: f32,
    audio_peak: f32,
    waterfall: VecDeque<Vec<f32>>,
    waterfall_capacity: usize,
    waterfall_dirty: bool,
    status: String,
    iq_frames: u64,
    audio_frames: u64,
}

impl ViewerState {
    fn new(waterfall_capacity: usize) -> Self {
        Self {
            iq_spectrum: Vec::new(),
            iq_rate: 0,
            audio_spectrum: Vec::new(),
            audio_rate: 0,
            audio_rms_db: -120.0,
            audio_peak: 0.0,
            waterfall: VecDeque::with_capacity(waterfall_capacity),
            waterfall_capacity,
            waterfall_dirty: false,
            status: "Niet verbonden".to_string(),
            iq_frames: 0,
            audio_frames: 0,
        }
    }
}

fn classify(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < TCI_HDR {
        return None;
    }
    let sample_rate = LittleEndian::read_u32(&data[4..8]);
    let stream_type = LittleEndian::read_u32(&data[24..28]);
    Some((stream_type, sample_rate))
}

fn read_f32(data: &[u8]) -> Vec<f32> {
    if data.len() <= TCI_HDR {
        return Vec::new();
    }
    let n = (data.len() - TCI_HDR) / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let off = TCI_HDR + i * 4;
        out.push(LittleEndian::read_f32(&data[off..off + 4]));
    }
    out
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|k| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * k as f32 / (n - 1) as f32).cos()))
        .collect()
}

fn iq_fft_db(iq: &[f32], fft: &dyn rustfft::Fft<f32>, n: usize, win: &[f32]) -> Vec<f32> {
    if iq.len() < n * 2 {
        return Vec::new();
    }
    let mut buf: Vec<Complex32> = Vec::with_capacity(n);
    for k in 0..n {
        let i = iq[k * 2] * win[k];
        let q = iq[k * 2 + 1] * win[k];
        buf.push(Complex32::new(i, q));
    }
    fft.process(&mut buf);
    let mut out = vec![0.0f32; n];
    for k in 0..n {
        let p = buf[k].norm_sqr() / (n as f32);
        let db = 10.0 * (p + 1e-12).log10();
        let shifted = (k + n / 2) % n;
        out[shifted] = db;
    }
    out
}

fn audio_fft_db(
    audio: &[f32],
    fft: &dyn rustfft::Fft<f32>,
    n: usize,
    win: &[f32],
) -> (Vec<f32>, f32, f32) {
    if audio.len() < n * 2 {
        return (Vec::new(), -120.0, 0.0);
    }
    let mut buf: Vec<Complex32> = Vec::with_capacity(n);
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;
    for k in 0..n {
        let m = (audio[k * 2] + audio[k * 2 + 1]) * 0.5;
        sum_sq += (m as f64) * (m as f64);
        peak = peak.max(m.abs());
        buf.push(Complex32::new(m * win[k], 0.0));
    }
    fft.process(&mut buf);
    let half = n / 2;
    let mut out = Vec::with_capacity(half);
    for k in 0..half {
        let p = buf[k].norm_sqr() / (n as f32);
        out.push(10.0 * (p + 1e-12).log10());
    }
    let rms = ((sum_sq / n as f64).sqrt() as f32).max(1e-9);
    let rms_db = 20.0 * rms.log10();
    (out, rms_db, peak)
}

async fn run_network(args: Args, state: Arc<Mutex<ViewerState>>, ctx: egui::Context) -> Result<()> {
    state.lock().unwrap().status = format!("Verbinden met {}…", args.connect);
    ctx.request_repaint();

    let (ws, _) = tokio_tungstenite::connect_async(&args.connect)
        .await
        .with_context(|| format!("Kon niet verbinden met {}", args.connect))?;
    let (mut tx, mut rx) = ws.split();

    state.lock().unwrap().status = format!("Verbonden met {}", args.connect);
    ctx.request_repaint();

    // Stuur de standaard init-sequentie tenzij --no-auto-start is opgegeven.
    // Dit is wat Thetis/Zeus nodig hebben om hun streams te beginnen.
    // (Op een tci-streamer client of fake-server hindert dit niet — die
    // accepteren of negeren de commando's.)
    if !args.no_auto_start {
        let init = [
            // Vraag basis-streams aan
            "audio_samplerate:48000;",
            "iq_samplerate:48000;",
            // Activeer ontvanger 0
            "rx_enable:0,true;",
            // De algemene start
            "audio_start:0;",
            "iq_start:0;",
            "start;",
        ];
        for cmd in init {
            if let Err(e) = tx.send(Message::Text(cmd.to_string())).await {
                state.lock().unwrap().status = format!("Init-fout: {}", e);
                ctx.request_repaint();
                return Err(e.into());
            }
        }
    }

    for c in &args.cmd {
        tx.send(Message::Text(c.clone())).await?;
    }

    let mut planner = FftPlanner::<f32>::new();
    let iq_fft = planner.plan_fft_forward(args.iq_fft);
    let audio_fft = planner.plan_fft_forward(args.audio_fft);
    let iq_win = hann(args.iq_fft);
    let audio_win = hann(args.audio_fft);

    let mut iq_accum: Vec<f32> = Vec::new();
    let mut audio_accum: Vec<f32> = Vec::new();

    while let Some(msg) = rx.next().await {
        match msg? {
            Message::Binary(data) => {
                let Some((stype, rate)) = classify(&data) else {
                    continue;
                };
                let samples = read_f32(&data);
                if samples.is_empty() {
                    continue;
                }
                match stype {
                    0 => {
                        iq_accum.extend_from_slice(&samples);
                        let max_keep = args.iq_fft * 4;
                        if iq_accum.len() > max_keep * 2 {
                            let drop = iq_accum.len() - max_keep * 2;
                            iq_accum.drain(..drop);
                        }
                        while iq_accum.len() >= args.iq_fft * 2 {
                            let block: Vec<f32> = iq_accum.drain(..args.iq_fft * 2).collect();
                            let spec = iq_fft_db(&block, &*iq_fft, args.iq_fft, &iq_win);
                            let mut s = state.lock().unwrap();
                            s.iq_rate = rate;
                            s.iq_frames += 1;
                            if s.waterfall.len() == s.waterfall_capacity {
                                s.waterfall.pop_back();
                            }
                            s.waterfall.push_front(spec.clone());
                            s.waterfall_dirty = true;
                            s.iq_spectrum = spec;
                        }
                        ctx.request_repaint();
                    }
                    1 => {
                        audio_accum.extend_from_slice(&samples);
                        let max_keep = args.audio_fft * 4;
                        if audio_accum.len() > max_keep * 2 {
                            let drop = audio_accum.len() - max_keep * 2;
                            audio_accum.drain(..drop);
                        }
                        while audio_accum.len() >= args.audio_fft * 2 {
                            let block: Vec<f32> = audio_accum.drain(..args.audio_fft * 2).collect();
                            let (spec, rms_db, peak) =
                                audio_fft_db(&block, &*audio_fft, args.audio_fft, &audio_win);
                            let mut s = state.lock().unwrap();
                            s.audio_rate = rate;
                            s.audio_frames += 1;
                            s.audio_spectrum = spec;
                            s.audio_rms_db = rms_db;
                            s.audio_peak = peak;
                        }
                        ctx.request_repaint();
                    }
                    _ => {}
                }
            }
            Message::Text(_) => {}
            Message::Close(_) => {
                state.lock().unwrap().status = "Verbinding gesloten".to_string();
                ctx.request_repaint();
                return Ok(());
            }
            _ => {}
        }
    }
    Ok(())
}

struct ViewerApp {
    state: Arc<Mutex<ViewerState>>,
    waterfall_tex: Option<TextureHandle>,
    waterfall_w: usize,
    waterfall_h: usize,
    db_min: f32,
    db_max: f32,
}

impl ViewerApp {
    fn new(state: Arc<Mutex<ViewerState>>, args: &Args) -> Self {
        let w = args.iq_fft.min(1024);
        let h = args.waterfall_rows;
        Self {
            state,
            waterfall_tex: None,
            waterfall_w: w,
            waterfall_h: h,
            db_min: -100.0,
            db_max: -20.0,
        }
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if self.waterfall_tex.is_none() {
            // epaint 0.33: ColorImage::new(size, Vec<Color32>) — geen fill-color meer.
            let pixels = vec![Color32::BLACK; self.waterfall_w * self.waterfall_h];
            let img = ColorImage::new([self.waterfall_w, self.waterfall_h], pixels);
            self.waterfall_tex = Some(ctx.load_texture("waterfall", img, TextureOptions::NEAREST));
        }
    }

    fn update_waterfall_texture(&mut self) {
        let Some(tex) = self.waterfall_tex.as_mut() else {
            return;
        };
        let mut s = self.state.lock().unwrap();
        if !s.waterfall_dirty || s.waterfall.is_empty() {
            return;
        }
        let pixels = vec![Color32::BLACK; self.waterfall_w * self.waterfall_h];
        let mut img = ColorImage::new([self.waterfall_w, self.waterfall_h], pixels);
        let db_min = self.db_min;
        let db_max = self.db_max;
        let w = self.waterfall_w;
        let h = self.waterfall_h;
        for (row_idx, row) in s.waterfall.iter().enumerate().take(h) {
            if row.is_empty() {
                continue;
            }
            for x in 0..w {
                let src = (x * row.len() / w).min(row.len() - 1);
                let v = row[src];
                let t = ((v - db_min) / (db_max - db_min)).clamp(0.0, 1.0);
                img.pixels[row_idx * w + x] = colormap(t);
            }
        }
        s.waterfall_dirty = false;
        drop(s);
        tex.set(img, TextureOptions::NEAREST);
    }
}

fn colormap(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.25 {
        let u = t / 0.25;
        (lerp(0.0, 0.0, u), lerp(0.0, 0.0, u), lerp(40.0, 120.0, u))
    } else if t < 0.5 {
        let u = (t - 0.25) / 0.25;
        (
            lerp(0.0, 0.0, u),
            lerp(0.0, 180.0, u),
            lerp(120.0, 180.0, u),
        )
    } else if t < 0.75 {
        let u = (t - 0.5) / 0.25;
        (
            lerp(0.0, 200.0, u),
            lerp(180.0, 220.0, u),
            lerp(180.0, 0.0, u),
        )
    } else {
        let u = (t - 0.75) / 0.25;
        (
            lerp(200.0, 255.0, u),
            lerp(220.0, 255.0, u),
            lerp(0.0, 200.0, u),
        )
    };
    Color32::from_rgb(r as u8, g as u8, b as u8)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_texture(ctx);
        self.update_waterfall_texture();

        let (status, iq_spec, iq_rate, audio_spec, audio_rate, audio_rms_db, audio_peak, iqf, af) = {
            let s = self.state.lock().unwrap();
            (
                s.status.clone(),
                s.iq_spectrum.clone(),
                s.iq_rate,
                s.audio_spectrum.clone(),
                s.audio_rate,
                s.audio_rms_db,
                s.audio_peak,
                s.iq_frames,
                s.audio_frames,
            )
        };

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(concat!("tci-viewer v", env!("CARGO_PKG_VERSION")))
                        .strong()
                        .color(egui::Color32::from_rgb(160, 200, 255)),
                );
                ui.separator();
                ui.label(&status);
                ui.separator();
                ui.label(format!("IQ: {} frames @ {} Hz", iqf, iq_rate));
                ui.separator();
                ui.label(format!("Audio: {} frames @ {} Hz", af, audio_rate));
                ui.separator();
                ui.label("dB:");
                ui.add(egui::Slider::new(&mut self.db_min, -160.0..=0.0).text("min"));
                ui.add(egui::Slider::new(&mut self.db_max, -80.0..=20.0).text("max"));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let total = ui.available_size();
            let half = (total.y - 8.0) / 2.0;

            ui.allocate_ui(egui::vec2(total.x, half), |ui| {
                let h_spec = half * 0.33;
                let h_wf = half - h_spec - 24.0;
                ui.label(egui::RichText::new(format!("IQ-spectrum ({} Hz)", iq_rate)).strong());
                draw_spectrum(ui, &iq_spec, h_spec, self.db_min, self.db_max);
                ui.label(egui::RichText::new("Waterfall (boven = nieuwste)").small());
                if let Some(tex) = &self.waterfall_tex {
                    let size = egui::vec2(ui.available_width(), h_wf);
                    ui.add(egui::Image::new((tex.id(), size)));
                }
            });

            ui.add_space(8.0);

            ui.allocate_ui(egui::vec2(total.x, half), |ui| {
                ui.label(
                    egui::RichText::new(format!("Audio-spectrum ({} Hz)", audio_rate)).strong(),
                );
                draw_spectrum(ui, &audio_spec, half * 0.7, self.db_min, self.db_max);

                ui.horizontal(|ui| {
                    ui.label(format!("RMS {:6.1} dB", audio_rms_db));
                    ui.label(format!("piek {:5.2}", audio_peak));
                    let frac = ((audio_rms_db + 60.0) / 60.0).clamp(0.0, 1.0);
                    let bar = ui.available_width().min(400.0);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(bar, 18.0), egui::Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, 2.0, Color32::from_gray(40));
                    let mut fill = rect;
                    fill.set_width(rect.width() * frac);
                    let color = if audio_rms_db > -6.0 {
                        Color32::from_rgb(220, 80, 60)
                    } else if audio_rms_db > -20.0 {
                        Color32::from_rgb(220, 200, 60)
                    } else {
                        Color32::from_rgb(80, 200, 100)
                    };
                    p.rect_filled(fill, 2.0, color);
                });
            });
        });
    }
}

fn draw_spectrum(ui: &mut egui::Ui, spec: &[f32], height: f32, db_min: f32, db_max: f32) {
    let w = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, height), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, Color32::from_gray(20));
    if spec.is_empty() {
        return;
    }

    // dB gridlijnen
    let mut db = (db_max / 10.0).floor() * 10.0;
    while db >= db_min {
        let t = (db - db_min) / (db_max - db_min);
        let y = rect.bottom() - t * rect.height();
        p.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(0.5, Color32::from_gray(50)),
        );
        db -= 10.0;
    }
    // Spectrum-lijn
    let n = spec.len();
    let cols = rect.width() as usize;
    if cols < 2 {
        return;
    }
    let mut pts = Vec::with_capacity(cols);
    for x in 0..cols {
        let from = (x * n / cols).min(n.saturating_sub(1));
        let to = ((x + 1) * n / cols).max(from + 1).min(n);
        let mut v = -200.0f32;
        for &s in &spec[from..to] {
            if s > v {
                v = s;
            }
        }
        let t = ((v - db_min) / (db_max - db_min)).clamp(0.0, 1.0);
        let y = rect.bottom() - t * rect.height();
        pts.push(egui::pos2(rect.left() + x as f32, y));
    }
    for i in 0..pts.len().saturating_sub(1) {
        p.line_segment(
            [pts[i], pts[i + 1]],
            egui::Stroke::new(1.5, Color32::from_rgb(240, 200, 80)),
        );
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let state = Arc::new(Mutex::new(ViewerState::new(args.waterfall_rows)));
    let net_state = state.clone();
    let net_args = args.clone();

    let (ctx_tx, ctx_rx) = std::sync::mpsc::channel::<egui::Context>();

    std::thread::spawn(move || {
        let ctx = match ctx_rx.recv() {
            Ok(c) => c,
            Err(_) => return,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            loop {
                if let Err(e) = run_network(net_args.clone(), net_state.clone(), ctx.clone()).await
                {
                    net_state.lock().unwrap().status = format!("Fout: {:#}. Reconnect over 3s…", e);
                    ctx.request_repaint();
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                } else {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        });
    });

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([700.0, 500.0])
            .with_title(concat!("tci-viewer v", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    let app_args = args.clone();
    eframe::run_native(
        "tci-viewer",
        native,
        Box::new(move |cc| {
            let _ = ctx_tx.send(cc.egui_ctx.clone());
            Ok(Box::new(ViewerApp::new(state, &app_args)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe fout: {e}"))?;
    Ok(())
}
