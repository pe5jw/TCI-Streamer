//! tci-streamer-launcher — grafische launcher voor de tci-streamer binaries.
//!
//! Vervangt de C# WinForms-launcher: nu in Rust met eframe, zodat alles met
//! één `cargo build` in één pakket komt.
//!
//! Functionaliteit:
//!   - Server/Client mode toggle
//!   - Alle CLI-opties als dropdowns/velden
//!   - Live command-line preview
//!   - Start/Stop met process spawning + log venster
//!   - Sla op als .cmd (Windows) of .sh (Linux) — of beide tegelijk
//!   - Laad een eerder opgeslagen script terug in de GUI

use anyhow::Result;
use eframe::egui;
use egui::Color32;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tci_streamer::launcher::*;

/// Maximum aantal regels dat we in het log-venster bewaren (om geheugen
/// niet onbeperkt te laten groeien bij lange runs).
const MAX_LOG_LINES: usize = 2000;

struct LauncherApp {
    settings: BridgeSettings,
    log: Arc<Mutex<Vec<String>>>,
    proc: Option<Child>,
    status: String,
    cmd_preview_cache: String,
}

impl LauncherApp {
    fn new() -> Self {
        let s = BridgeSettings::default();
        let preview = build_command_line(&s, true);
        Self {
            settings: s,
            log: Arc::new(Mutex::new(Vec::new())),
            proc: None,
            status: "Gereed.".into(),
            cmd_preview_cache: preview,
        }
    }

    fn log_line(&self, msg: &str) {
        let stamp = chrono_like_stamp();
        let line = format!("[{}]  {}", stamp, msg);
        let mut log = self.log.lock().unwrap();
        log.push(line);
        if log.len() > MAX_LOG_LINES {
            let drop = log.len() - MAX_LOG_LINES;
            log.drain(..drop);
        }
    }

    fn refresh_preview(&mut self) {
        self.cmd_preview_cache = build_command_line(&self.settings, true);
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.proc.is_some() {
            self.status = "Draait al.".into();
            return;
        }
        let exe = PathBuf::from(&self.settings.exe_dir).join(exe_name(self.settings.role));
        if !exe.exists() {
            let msg = format!("FOUT: niet gevonden: {}", exe.display());
            self.log_line(&msg);
            self.status = "Executable niet gevonden — check de programma-map.".into();
            return;
        }
        let args = build_args(&self.settings);
        let mut cmd = Command::new(&exe);
        cmd.args(&args)
            .current_dir(&self.settings.exe_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: voorkomt dat er een extra cmd-venster verschijnt.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.log_line(&format!("FOUT bij starten: {}", e));
                return;
            }
        };

        self.log_line(&format!(
            "Gestart: {} {}",
            exe_name(self.settings.role),
            build_command_line(&self.settings, false)
        ));

        // Threads die stdout/stderr van het kind-proces lezen en in de log
        // duwen. We klonen Arc<Mutex<…>> per thread; egui-repaints triggeren
        // we via ctx.request_repaint() vanuit elke regel.
        if let Some(out) = child.stdout.take() {
            let log = self.log.clone();
            let ctx2 = ctx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(out);
                for line in reader.lines().flatten() {
                    push_log(&log, &line);
                    ctx2.request_repaint();
                }
            });
        }
        if let Some(err) = child.stderr.take() {
            let log = self.log.clone();
            let ctx2 = ctx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(err);
                for line in reader.lines().flatten() {
                    push_log(&log, &line);
                    ctx2.request_repaint();
                }
            });
        }

        self.proc = Some(child);
        self.status = format!(
            "{} draait.",
            if self.settings.role == BridgeRole::Server { "Server" } else { "Client" }
        );
    }

    fn stop(&mut self) {
        if let Some(mut p) = self.proc.take() {
            let _ = p.kill();
            let _ = p.wait();
            self.log_line("--- proces gestopt ---");
            self.status = "Gestopt.".into();
        }
    }

    fn check_proc_exited(&mut self) {
        if let Some(p) = self.proc.as_mut() {
            if let Ok(Some(status)) = p.try_wait() {
                self.log_line(&format!("--- proces afgesloten (exit {}) ---", status));
                self.proc = None;
                self.status = "Gestopt.".into();
            }
        }
    }

    fn save_script(&mut self) {
        let (default_name, default_ext) = match self.settings.role {
            BridgeRole::Server => ("start-server", "cmd"),
            BridgeRole::Client => ("start-client", "cmd"),
        };
        let dlg = rfd::FileDialog::new()
            .set_title("Sla start-script op")
            .add_filter("Windows batch (.cmd)", &["cmd"])
            .add_filter("Linux shell (.sh)", &["sh"])
            .set_file_name(&format!("{}.{}", default_name, default_ext))
            .save_file();
        let Some(mut path) = dlg else { return; };

        // Bepaal het formaat aan de extensie. Als de gebruiker geen extensie
        // typte, vallen we terug op .cmd op Windows en .sh elders.
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let (body, ext_used) = match ext.as_str() {
            "sh" => (build_sh_script(&self.settings), "sh"),
            "cmd" | "bat" => (build_cmd_script(&self.settings), "cmd"),
            _ => {
                // Geen herkenbare extensie — gebruik platform-default.
                let default = if cfg!(windows) { "cmd" } else { "sh" };
                path.set_extension(default);
                let body = if default == "cmd" {
                    build_cmd_script(&self.settings)
                } else {
                    build_sh_script(&self.settings)
                };
                (body, default)
            }
        };

        match std::fs::write(&path, &body) {
            Ok(_) => {
                // Op Unix: .sh moet executable zijn.
                #[cfg(unix)]
                if ext_used == "sh" {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&path) {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        let _ = std::fs::set_permissions(&path, perms);
                    }
                }
                let _ = ext_used; // suppress unused on windows
                self.log_line(&format!("Script opgeslagen: {}", path.display()));
                self.status = format!("Opgeslagen: {}", path.display());
            }
            Err(e) => {
                self.log_line(&format!("FOUT bij opslaan: {}", e));
            }
        }
    }

    /// Sla zowel een .cmd als een .sh op met dezelfde basis-naam, in één map.
    fn save_both_scripts(&mut self) {
        let basename = match self.settings.role {
            BridgeRole::Server => "start-server",
            BridgeRole::Client => "start-client",
        };
        let dlg = rfd::FileDialog::new()
            .set_title("Kies map voor start-scripts (.cmd + .sh)")
            .pick_folder();
        let Some(dir) = dlg else { return; };
        let cmd_path = dir.join(format!("{}.cmd", basename));
        let sh_path = dir.join(format!("{}.sh", basename));
        let mut ok = 0;
        let mut errs = Vec::new();
        match std::fs::write(&cmd_path, build_cmd_script(&self.settings)) {
            Ok(_) => ok += 1,
            Err(e) => errs.push(format!("{}: {}", cmd_path.display(), e)),
        }
        match std::fs::write(&sh_path, build_sh_script(&self.settings)) {
            Ok(_) => {
                ok += 1;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&sh_path) {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        let _ = std::fs::set_permissions(&sh_path, perms);
                    }
                }
            }
            Err(e) => errs.push(format!("{}: {}", sh_path.display(), e)),
        }
        if ok > 0 {
            self.log_line(&format!("{} script(s) opgeslagen in {}", ok, dir.display()));
            self.status = format!("Scripts opgeslagen in {}", dir.display());
        }
        for e in errs {
            self.log_line(&format!("FOUT: {}", e));
        }
    }

    /// Lees een eerder opgeslagen .cmd of .sh script in en vul de velden.
    fn load_script(&mut self) {
        let dlg = rfd::FileDialog::new()
            .set_title("Laad start-script")
            .add_filter("Start-scripts", &["cmd", "sh", "bat"])
            .add_filter("Alle bestanden", &["*"])
            .pick_file();
        let Some(path) = dlg else { return; };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                self.log_line(&format!("FOUT bij lezen: {}", e));
                return;
            }
        };
        match parse_script(&content) {
            Ok(parsed) => {
                // Bewaar exe_dir uit huidige settings — die hoort niet bij
                // het script (scripts gebruiken cd /d "%~dp0" als impliciete map).
                let exe_dir = std::mem::take(&mut self.settings.exe_dir);
                self.settings = parsed;
                self.settings.exe_dir = exe_dir;
                self.refresh_preview();
                self.log_line(&format!("Script ingelezen: {}", path.display()));
                self.status = format!("Geladen: {}", path.display());
            }
            Err(e) => {
                self.log_line(&format!("FOUT bij parsen van {}: {}", path.display(), e));
                self.status = format!("Kon script niet parsen: {}", e);
            }
        }
    }

    fn browse_exe_dir(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("Map met tci-streamer binaries")
            .pick_folder()
        {
            self.settings.exe_dir = p.to_string_lossy().to_string();
            self.refresh_preview();
        }
    }
}

fn push_log(log: &Arc<Mutex<Vec<String>>>, line: &str) {
    let stamp = chrono_like_stamp();
    let formatted = format!("[{}]  {}", stamp, line);
    let mut l = log.lock().unwrap();
    l.push(formatted);
    if l.len() > MAX_LOG_LINES {
        let drop = l.len() - MAX_LOG_LINES;
        l.drain(..drop);
    }
}

fn chrono_like_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = now.as_secs();
    let h = ((total_secs / 3600) % 24) as u32;
    let m = ((total_secs / 60) % 60) as u32;
    let s = (total_secs % 60) as u32;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_proc_exited();

        // Herhaal-tekenen elke 500ms zodat status (proc-exit) verfrist.
        ctx.request_repaint_after(Duration::from_millis(500));

        let running = self.proc.is_some();
        let mut changed = false;

        egui::TopBottomPanel::top("mode").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(
                    concat!("tci-streamer launcher v", env!("CARGO_PKG_VERSION"))
                ).strong().color(Color32::from_rgb(160, 200, 255)));
                ui.separator();
                ui.label("Modus:");
                if ui.add_enabled(!running, egui::RadioButton::new(
                    self.settings.role == BridgeRole::Client, "Client (bedien-PC)")).clicked() {
                    self.settings.role = BridgeRole::Client;
                    changed = true;
                }
                if ui.add_enabled(!running, egui::RadioButton::new(
                    self.settings.role == BridgeRole::Server, "Server (radio-PC)")).clicked() {
                    self.settings.role = BridgeRole::Server;
                    changed = true;
                }
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(&self.status).color(Color32::from_gray(170)).small());
            ui.add_space(2.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.settings.role == BridgeRole::Server {
                    changed |= server_panel(ui, &mut self.settings);
                } else {
                    changed |= client_panel(ui, &mut self.settings);
                }

                ui.add_space(8.0);
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Programma-map").strong());
                    ui.horizontal(|ui| {
                        if ui.text_edit_singleline(&mut self.settings.exe_dir).changed() {
                            changed = true;
                        }
                        if ui.button("Bladeren…").clicked() {
                            self.browse_exe_dir();
                        }
                    });
                });

                ui.add_space(8.0);
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Command-line (headless)").strong());
                    let mut preview = self.cmd_preview_cache.clone();
                    ui.add(egui::TextEdit::multiline(&mut preview)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .interactive(false));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Kopieer").clicked() {
                            ctx.copy_text(self.cmd_preview_cache.clone());
                            self.status = "Gekopieerd.".into();
                        }
                        if ui.button("Sla op…").on_hover_text(
                            "Sla op als .cmd (Windows) of .sh (Linux). Extensie bepaalt het formaat."
                        ).clicked() {
                            self.save_script();
                        }
                        if ui.button("Sla beide op…").on_hover_text(
                            "Kies een map; .cmd én .sh worden allebei opgeslagen."
                        ).clicked() {
                            self.save_both_scripts();
                        }
                        if ui.button("Laad…").on_hover_text(
                            "Lees een eerder opgeslagen .cmd of .sh script in."
                        ).clicked() {
                            self.load_script();
                        }
                    });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let start_btn = egui::Button::new(
                        egui::RichText::new("▶ Start").size(16.0))
                        .fill(Color32::from_rgb(40, 90, 40))
                        .min_size(egui::vec2(150.0, 36.0));
                    if ui.add_enabled(!running, start_btn).clicked() {
                        self.start(ctx);
                    }
                    let stop_btn = egui::Button::new(
                        egui::RichText::new("⏹ Stop").size(16.0))
                        .fill(Color32::from_rgb(90, 40, 40))
                        .min_size(egui::vec2(150.0, 36.0));
                    if ui.add_enabled(running, stop_btn).clicked() {
                        self.stop();
                    }
                });

                ui.add_space(8.0);
                ui.label(egui::RichText::new("Log").strong());
                let log_lines = self.log.lock().unwrap().clone();
                let log_text: String = log_lines.join("\n");
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let mut log_view = log_text;
                        ui.add_sized(
                            [ui.available_width(), 240.0],
                            egui::TextEdit::multiline(&mut log_view)
                                .font(egui::TextStyle::Monospace)
                                .interactive(false),
                        );
                    });
            });
        });

        if changed {
            self.refresh_preview();
        }
    }
}

// ── Sub-panels ──────────────────────────────────────────────────────────────

fn server_panel(ui: &mut egui::Ui, s: &mut BridgeSettings) -> bool {
    let mut changed = false;
    ui.group(|ui| {
        ui.label(egui::RichText::new("Server-instellingen").strong()
            .color(Color32::from_rgb(160, 200, 255)));
        egui::Grid::new("server_grid")
            .num_columns(2).spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Upstream (Thetis/Zeus):");
                changed |= ui.text_edit_singleline(&mut s.upstream).changed();
                ui.end_row();

                ui.label("Luister-adres (clients):");
                changed |= ui.text_edit_singleline(&mut s.server_listen).changed();
                ui.end_row();

                ui.label("IQ-swap (Zeus/Thetis):");
                changed |= iq_swap_combo(ui, &mut s.iq_swap);
                ui.end_row();

                ui.label("IQ target rate:");
                changed |= ui.add(egui::DragValue::new(&mut s.iq_target_rate)
                    .range(8000..=192000).speed(1000)).changed();
                ui.end_row();

                ui.label("FFT size:");
                changed |= ui.add(egui::DragValue::new(&mut s.fft_size)
                    .range(256..=16384).speed(1)).changed();
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
            changed |= ui.checkbox(&mut s.passthrough, "Passthrough (diagnose)").changed();
            changed |= ui.checkbox(&mut s.log_tci_commands, "Log TCI commando's").changed();
            changed |= ui.checkbox(&mut s.auto_start, "Auto-start streams")
                .on_hover_text("Stuurt audio_start/iq_start/start naar upstream zodra verbonden. \
                                Aanzetten als je zonder THRA wilt testen (bv. met alleen tci-viewer). \
                                Uit laten als THRA al de regie heeft.").changed();
        });
    });
    changed
}

fn client_panel(ui: &mut egui::Ui, s: &mut BridgeSettings) -> bool {
    let mut changed = false;
    ui.group(|ui| {
        ui.label(egui::RichText::new("Verbinding").strong()
            .color(Color32::from_rgb(160, 200, 255)));
        egui::Grid::new("client_conn").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Server URL:");
            changed |= ui.text_edit_singleline(&mut s.server).changed();
            ui.end_row();
            ui.label("Lokaal luister-adres:");
            changed |= ui.text_edit_singleline(&mut s.client_listen).changed();
            ui.end_row();
        });
    });

    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(egui::RichText::new("Audio codec").strong()
            .color(Color32::from_rgb(160, 200, 255)));
        let opus = matches!(s.codec, Codec::Opus);
        let opus_or_flac = matches!(s.codec, Codec::Opus | Codec::Flac);
        egui::Grid::new("audio_grid").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Codec:");
            changed |= codec_combo(ui, &mut s.codec);
            ui.label("Kanalen:");
            ui.add_enabled_ui(opus_or_flac, |ui| {
                changed |= channels_combo(ui, &mut s.channels);
            });
            ui.end_row();

            ui.label("Sample rate:");
            ui.add_enabled_ui(opus, |ui| {
                changed |= sample_rate_combo(ui, &mut s.sample_rate);
            });
            ui.label("Frame (ms):");
            ui.add_enabled_ui(opus, |ui| {
                changed |= frame_ms_combo(ui, &mut s.frame_ms);
            });
            ui.end_row();

            ui.label("Bitrate:");
            ui.add_enabled_ui(opus, |ui| {
                changed |= bitrate_combo(ui, &mut s.bitrate);
            });
            ui.label("");
            ui.label("");
            ui.end_row();

            ui.label("RX filter:");
            changed |= filter_combo(ui, &mut s.rx_filter, "rx_filter");
            ui.label("TX filter:");
            changed |= filter_combo(ui, &mut s.tx_filter, "tx_filter");
            ui.end_row();
        });
    });

    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(egui::RichText::new("IQ / spectrum").strong()
            .color(Color32::from_rgb(160, 200, 255)));
        egui::Grid::new("iq_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("IQ mode:");
            changed |= iq_mode_combo(ui, &mut s.iq_mode);
            ui.end_row();
        });
    });
    changed
}

// ── Combo helpers ──

fn iq_mode_combo(ui: &mut egui::Ui, v: &mut IqMode) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("iq_mode").selected_text(iq_mode_arg(*v))
        .show_ui(ui, |ui| {
            for m in [IqMode::Spectrum, IqMode::DecimatedIq, IqMode::Disabled] {
                changed |= ui.selectable_value(v, m, iq_mode_arg(m)).changed();
            }
        });
    changed
}

fn iq_swap_combo(ui: &mut egui::Ui, v: &mut IqSwap) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("iq_swap").selected_text(iq_swap_arg(*v))
        .show_ui(ui, |ui| {
            for m in [IqSwap::None, IqSwap::Swap, IqSwap::Conj] {
                changed |= ui.selectable_value(v, m, iq_swap_arg(m)).changed();
            }
        });
    changed
}

fn codec_combo(ui: &mut egui::Ui, v: &mut Codec) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("codec").selected_text(codec_arg(*v))
        .show_ui(ui, |ui| {
            for m in [Codec::Opus, Codec::Flac, Codec::Lossless, Codec::LosslessInt16] {
                changed |= ui.selectable_value(v, m, codec_arg(m)).changed();
            }
        });
    changed
}

fn channels_combo(ui: &mut egui::Ui, v: &mut Channels) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("channels").selected_text(channels_arg(*v))
        .show_ui(ui, |ui| {
            for m in [Channels::Mono, Channels::Stereo] {
                changed |= ui.selectable_value(v, m, channels_arg(m)).changed();
            }
        });
    changed
}

fn filter_combo(ui: &mut egui::Ui, v: &mut FilterPreset, id: &str) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(filter_arg(*v))
        .show_ui(ui, |ui| {
            for m in [FilterPreset::Off, FilterPreset::Wide, FilterPreset::Voice,
                      FilterPreset::Ssb, FilterPreset::Narrow] {
                changed |= ui.selectable_value(v, m, filter_arg(m)).changed();
            }
        });
    changed
}

fn sample_rate_combo(ui: &mut egui::Ui, v: &mut u32) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("sample_rate").selected_text(v.to_string())
        .show_ui(ui, |ui| {
            for &r in &[8000u32, 12000, 16000, 24000, 48000] {
                changed |= ui.selectable_value(v, r, r.to_string()).changed();
            }
        });
    changed
}

fn frame_ms_combo(ui: &mut egui::Ui, v: &mut f32) -> bool {
    let mut changed = false;
    let label = if (*v - v.floor()).abs() < 0.001 {
        format!("{}", *v as i32)
    } else {
        format!("{}", v)
    };
    egui::ComboBox::from_id_salt("frame_ms").selected_text(label)
        .show_ui(ui, |ui| {
            for &r in &[2.5f32, 5.0, 10.0, 20.0, 40.0, 60.0] {
                let label = if (r - r.floor()).abs() < 0.001 {
                    format!("{}", r as i32)
                } else {
                    format!("{}", r)
                };
                changed |= ui.selectable_value(v, r, label).changed();
            }
        });
    changed
}

fn bitrate_combo(ui: &mut egui::Ui, v: &mut u32) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("bitrate").selected_text(v.to_string())
        .show_ui(ui, |ui| {
            for &r in &[16000u32, 24000, 32000, 48000, 64000, 96000] {
                changed |= ui.selectable_value(v, r, r.to_string()).changed();
            }
        });
    changed
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 820.0])
            .with_min_inner_size([560.0, 600.0])
            .with_title(concat!("tci-streamer launcher v", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    eframe::run_native(
        "tci-streamer-launcher",
        native,
        Box::new(|_cc| Ok(Box::new(LauncherApp::new()))),
    ).map_err(|e| anyhow::anyhow!("eframe fout: {e}"))?;
    Ok(())
}
