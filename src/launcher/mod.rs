//! Launcher: instellingen-model + command-line builder voor de tci-streamer
//! binaries. Eén bron van waarheid die zowel de GUI vult als de
//! script-export gebruikt — zodat ze gegarandeerd identiek zijn.

use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeRole { Server, Client }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IqMode { Spectrum, DecimatedIq, Disabled }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IqSwap { None, Swap, Conj }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec { Opus, Flac, Lossless, LosslessInt16 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channels { Mono, Stereo }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterPreset { Off, Wide, Voice, Ssb, Narrow }

/// Alle instellingen voor één bridge-instance (server of client).
#[derive(Debug, Clone)]
pub struct BridgeSettings {
    pub role: BridgeRole,

    // Server
    pub upstream: String,
    pub server_listen: String,
    pub iq_target_rate: u32,
    pub fft_size: u32,
    pub iq_swap: IqSwap,
    pub passthrough: bool,
    pub log_tci_commands: bool,
    pub auto_start: bool,

    // Client
    pub server: String,
    pub client_listen: String,
    pub iq_mode: IqMode,
    pub codec: Codec,
    pub sample_rate: u32,
    pub frame_ms: f32,
    pub bitrate: u32,
    pub channels: Channels,
    pub spectrum_bins: u32,
    pub spectrum_fps: u32,
    pub rx_filter: FilterPreset,
    pub tx_filter: FilterPreset,

    /// Map waar de tci-streamer .exe's staan.
    pub exe_dir: String,
}

impl Default for BridgeSettings {
    fn default() -> Self {
        Self {
            role: BridgeRole::Client,
            upstream: "ws://127.0.0.1:40001".into(),
            server_listen: "0.0.0.0:9001".into(),
            iq_target_rate: 48000,
            fft_size: 4096,
            iq_swap: IqSwap::None,
            passthrough: false,
            log_tci_commands: false,
            auto_start: false,
            server: "ws://127.0.0.1:9001".into(),
            client_listen: "127.0.0.1:40002".into(),
            iq_mode: IqMode::Spectrum,
            codec: Codec::Opus,
            sample_rate: 48000,
            frame_ms: 20.0,
            bitrate: 24000,
            channels: Channels::Mono,
            spectrum_bins: 2048,
            spectrum_fps: 20,
            rx_filter: FilterPreset::Off,
            tx_filter: FilterPreset::Off,
            exe_dir: ".".into(),
        }
    }
}

// Expliciete CLI-string mappings — moeten exact matchen met wat clap in
// de Rust binaries accepteert.

pub fn iq_mode_arg(m: IqMode) -> &'static str {
    match m {
        IqMode::Spectrum => "spectrum",
        IqMode::DecimatedIq => "decimated-iq",
        IqMode::Disabled => "disabled",
    }
}

pub fn iq_swap_arg(s: IqSwap) -> &'static str {
    match s {
        IqSwap::None => "none",
        IqSwap::Swap => "swap",
        IqSwap::Conj => "conj",
    }
}

pub fn codec_arg(c: Codec) -> &'static str {
    match c {
        Codec::Opus => "opus",
        Codec::Flac => "flac",
        Codec::Lossless => "lossless",
        Codec::LosslessInt16 => "lossless-int16",
    }
}

pub fn channels_arg(c: Channels) -> &'static str {
    match c {
        Channels::Mono => "mono",
        Channels::Stereo => "stereo",
    }
}

pub fn filter_arg(f: FilterPreset) -> &'static str {
    match f {
        FilterPreset::Off => "off",
        FilterPreset::Wide => "wide",
        FilterPreset::Voice => "voice",
        FilterPreset::Ssb => "ssb",
        FilterPreset::Narrow => "narrow",
    }
}

pub fn exe_name(role: BridgeRole) -> &'static str {
    match role {
        BridgeRole::Server => {
            if cfg!(windows) { "tci-streamer-server.exe" } else { "tci-streamer-server" }
        }
        BridgeRole::Client => {
            if cfg!(windows) { "tci-streamer-client.exe" } else { "tci-streamer-client" }
        }
    }
}

fn fmt_frame_ms(v: f32) -> String {
    if (v - v.floor()).abs() < 0.001 {
        format!("{}", v as i32)
    } else {
        format!("{}", v) // f32 default formatter geeft "2.5" voor 2.5
    }
}

/// Bouw de argumentenlijst (zonder de exe-naam zelf).
pub fn build_args(s: &BridgeSettings) -> Vec<String> {
    let mut a = Vec::new();
    if s.role == BridgeRole::Server {
        a.push("--upstream".into()); a.push(s.upstream.clone());
        a.push("--listen".into());   a.push(s.server_listen.clone());
        a.push("--iq-target-rate".into()); a.push(s.iq_target_rate.to_string());
        a.push("--fft-size".into());       a.push(s.fft_size.to_string());
        a.push("--iq-swap".into());        a.push(iq_swap_arg(s.iq_swap).into());
        if s.passthrough { a.push("--passthrough".into()); }
        if s.log_tci_commands { a.push("--log-tci-commands".into()); }
        if s.auto_start { a.push("--auto-start".into()); }
    } else {
        a.push("--server".into()); a.push(s.server.clone());
        a.push("--listen".into()); a.push(s.client_listen.clone());
        a.push("--iq-mode".into()); a.push(iq_mode_arg(s.iq_mode).into());
        a.push("--codec".into());   a.push(codec_arg(s.codec).into());
        a.push("--sample-rate".into()); a.push(s.sample_rate.to_string());
        a.push("--frame-ms".into());    a.push(fmt_frame_ms(s.frame_ms));
        a.push("--bitrate".into());     a.push(s.bitrate.to_string());
        a.push("--channels".into());    a.push(channels_arg(s.channels).into());
        a.push("--spectrum-bins".into()); a.push(s.spectrum_bins.to_string());
        a.push("--spectrum-fps".into());  a.push(s.spectrum_fps.to_string());
        a.push("--rx-filter".into()); a.push(filter_arg(s.rx_filter).into());
        a.push("--tx-filter".into()); a.push(filter_arg(s.tx_filter).into());
    }
    a
}

fn quote_if_needed(s: &str) -> String {
    if s.contains(' ') || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Volledige command-line als string voor weergave/export.
pub fn build_command_line(s: &BridgeSettings, include_exe: bool) -> String {
    let mut out = String::new();
    if include_exe {
        out.push_str(exe_name(s.role));
    }
    for arg in build_args(s) {
        if !out.is_empty() { out.push(' '); }
        let _ = write!(out, "{}", quote_if_needed(&arg));
    }
    out
}

// ── Script generatie ────────────────────────────────────────────────────────

/// Bouw een Windows .cmd start-script.
pub fn build_cmd_script(s: &BridgeSettings) -> String {
    let mut out = String::new();
    out.push_str("@echo off\r\n");
    out.push_str("REM Gegenereerd door tci-streamer-launcher\r\n");
    out.push_str(&format!("REM Rol: {:?}\r\n", s.role));
    out.push_str("\r\n");
    out.push_str("cd /d \"%~dp0\"\r\n");
    out.push_str(&build_command_line(s, true));
    out.push_str("\r\npause\r\n");
    out
}

/// Bouw een Linux .sh start-script.
pub fn build_sh_script(s: &BridgeSettings) -> String {
    let mut out = String::new();
    out.push_str("#!/usr/bin/env bash\n");
    out.push_str("# Gegenereerd door tci-streamer-launcher\n");
    out.push_str(&format!("# Rol: {:?}\n", s.role));
    out.push_str("set -e\n");
    out.push_str("cd \"$(dirname \"$0\")\"\n");
    // Linux gebruikt geen .exe-extensie.
    let exe = match s.role {
        BridgeRole::Server => "./tci-streamer-server",
        BridgeRole::Client => "./tci-streamer-client",
    };
    out.push_str(exe);
    for arg in build_args(s) {
        out.push(' ');
        out.push_str(&quote_if_needed(&arg));
    }
    out.push('\n');
    out
}

// ── Script parsing ──────────────────────────────────────────────────────────

/// Lees een .cmd of .sh script en haal de tci-streamer aanroep eruit.
/// Variabelen (%NAME% in .cmd, $NAME / ${NAME} in .sh) worden geëxpandeerd
/// op basis van `set`/`NAME=...` regels eerder in het script.
///
/// Retourneert een nieuwe `BridgeSettings` met de gevonden waarden, of
/// een foutmelding als er geen herkenbare aanroep is.
pub fn parse_script(content: &str) -> Result<BridgeSettings, String> {
    let mut vars: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // Vind line continuations (^ in .cmd aan eind van regel, \ in .sh) en
    // plak die regels samen voordat we tokeniseren.
    let joined = join_continuations(content);

    let mut cmdline: Option<String> = None;
    for raw in joined.lines() {
        let line = raw.trim();
        if line.is_empty() { continue; }
        if line.starts_with('#') || line.starts_with("REM ") || line.starts_with("rem ")
           || line == "REM" || line == "rem" || line.starts_with("::") {
            continue;
        }
        // set NAME=VALUE   (Windows)
        if let Some(rest) = strip_prefix_ci(line, "set ") {
            if let Some((k, v)) = rest.split_once('=') {
                vars.insert(k.trim().to_string(), v.trim().to_string());
                continue;
            }
        }
        // NAME=VALUE       (Linux, simpel formaat zonder export)
        // Heuristiek: regel zonder spaties voor het =-teken en het =-teken
        // staat vroeg, geen $ of % erin.
        if !line.starts_with("./") && !line.contains(" ") {
            if let Some((k, v)) = line.split_once('=') {
                if !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let val = v.trim().trim_matches('"').trim_matches('\'');
                    vars.insert(k.to_string(), val.to_string());
                    continue;
                }
            }
        }
        // Skip overige boilerplate
        if line.starts_with("@echo")
           || line.starts_with("cd ") || line.starts_with("cd/")
           || line.starts_with("pause")
           || line.starts_with("set -")
           || line.starts_with("#!")
           || line.starts_with("export ") {
            continue;
        }
        // Aanroep-regel: bevat de naam van de binary.
        if line.contains("tci-streamer-server") || line.contains("tci-streamer-client") {
            cmdline = Some(line.to_string());
            break;
        }
    }

    let cmdline = cmdline.ok_or("Geen tci-streamer aanroep gevonden in script.".to_string())?;
    let expanded = expand_vars(&cmdline, &vars);
    parse_command_line(&expanded)
}

/// Plak regels die eindigen op ^ (cmd) of \ (sh) aan de volgende regel.
fn join_continuations(s: &str) -> String {
    let mut out = String::new();
    let mut iter = s.lines().peekable();
    while let Some(line) = iter.next() {
        let trimmed = line.trim_end();
        if trimmed.ends_with('^') {
            out.push_str(trimmed.trim_end_matches('^'));
            out.push(' ');
        } else if trimmed.ends_with('\\') && !trimmed.ends_with("\\\\") {
            // niet absoluut waterdicht (escaped backslash), maar genoeg
            // voor de scripts die wij genereren.
            out.push_str(trimmed.trim_end_matches('\\'));
            out.push(' ');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Expandeer %NAME% en $NAME / ${NAME} referenties met waarden uit `vars`.
/// Onbekende variabelen blijven staan zoals ze waren.
fn expand_vars(s: &str, vars: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '%' {
            // %NAME%
            if let Some(end) = s[i + 1..].find('%') {
                let name = &s[i + 1..i + 1 + end];
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    if let Some(v) = vars.get(name) {
                        out.push_str(v);
                        i += end + 2;
                        continue;
                    }
                }
            }
        } else if c == '$' && i + 1 < bytes.len() {
            // ${NAME} of $NAME
            if bytes[i + 1] as char == '{' {
                if let Some(end) = s[i + 2..].find('}') {
                    let name = &s[i + 2..i + 2 + end];
                    if let Some(v) = vars.get(name) {
                        out.push_str(v);
                        i += end + 3;
                        continue;
                    }
                }
            } else {
                let mut j = i + 1;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_')
                { j += 1; }
                if j > i + 1 {
                    let name = &s[i + 1..j];
                    if let Some(v) = vars.get(name) {
                        out.push_str(v);
                        i = j;
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Tokeniseer een command-line en vul een BridgeSettings.
fn parse_command_line(line: &str) -> Result<BridgeSettings, String> {
    let tokens = tokenize(line);
    if tokens.is_empty() {
        return Err("Lege command-line.".into());
    }
    // Bepaal rol uit de eerste token (de exe-naam, ergens in een pad).
    let exe = &tokens[0];
    let role = if exe.contains("tci-streamer-server") {
        BridgeRole::Server
    } else if exe.contains("tci-streamer-client") {
        BridgeRole::Client
    } else {
        return Err(format!("Onbekende binary: {}", exe));
    };

    let mut s = BridgeSettings::default();
    s.role = role;

    let mut i = 1;
    while i < tokens.len() {
        let key = tokens[i].as_str();
        let val = tokens.get(i + 1).map(|s| s.as_str());
        let took_val = match key {
            // Booleans (geen waarde)
            "--passthrough" => { s.passthrough = true; false }
            "--log-tci-commands" => { s.log_tci_commands = true; false }
            "--auto-start" => { s.auto_start = true; false }
            // Server
            "--upstream" => { s.upstream = req(val, key)?.to_string(); true }
            "--listen" => {
                let v = req(val, key)?.to_string();
                // --listen wordt door zowel server als client gebruikt.
                match role {
                    BridgeRole::Server => s.server_listen = v,
                    BridgeRole::Client => s.client_listen = v,
                }
                true
            }
            "--iq-target-rate" => { s.iq_target_rate = req_parse(val, key)?; true }
            "--fft-size" => { s.fft_size = req_parse(val, key)?; true }
            "--iq-swap" => { s.iq_swap = parse_iq_swap(req(val, key)?)?; true }
            // Client
            "--server" => { s.server = req(val, key)?.to_string(); true }
            "--iq-mode" => { s.iq_mode = parse_iq_mode(req(val, key)?)?; true }
            "--codec" => { s.codec = parse_codec(req(val, key)?)?; true }
            "--sample-rate" => { s.sample_rate = req_parse(val, key)?; true }
            "--frame-ms" => {
                let v = req(val, key)?;
                s.frame_ms = v.parse().map_err(|_| format!("Ongeldige frame-ms: {}", v))?;
                true
            }
            "--bitrate" => { s.bitrate = req_parse(val, key)?; true }
            "--channels" => { s.channels = parse_channels(req(val, key)?)?; true }
            "--spectrum-bins" => { s.spectrum_bins = req_parse(val, key)?; true }
            "--spectrum-fps" => { s.spectrum_fps = req_parse(val, key)?; true }
            "--rx-filter" => { s.rx_filter = parse_filter(req(val, key)?)?; true }
            "--tx-filter" => { s.tx_filter = parse_filter(req(val, key)?)?; true }
            // Onbekend: skip stilletjes (forwards-compatible)
            _ if key.starts_with("--") => {
                // Onbekende optie met waarde — probeer er één over te slaan.
                // We weten niet of het een flag of een key=value is; voor
                // veiligheid: skip alleen de key.
                false
            }
            _ => false,
        };
        i += if took_val { 2 } else { 1 };
    }

    Ok(s)
}

fn req<'a>(v: Option<&'a str>, key: &str) -> Result<&'a str, String> {
    v.ok_or_else(|| format!("Ontbrekende waarde voor {}", key))
}
fn req_parse<T: std::str::FromStr>(v: Option<&str>, key: &str) -> Result<T, String> {
    let v = req(v, key)?;
    v.parse::<T>().map_err(|_| format!("Ongeldige waarde voor {}: {}", key, v))
}
fn parse_iq_swap(v: &str) -> Result<IqSwap, String> {
    match v { "none" => Ok(IqSwap::None), "swap" => Ok(IqSwap::Swap), "conj" => Ok(IqSwap::Conj),
              _ => Err(format!("Onbekende iq-swap: {}", v)) }
}
fn parse_iq_mode(v: &str) -> Result<IqMode, String> {
    match v { "spectrum" => Ok(IqMode::Spectrum), "decimated-iq" => Ok(IqMode::DecimatedIq),
              "disabled" => Ok(IqMode::Disabled), _ => Err(format!("Onbekende iq-mode: {}", v)) }
}
fn parse_codec(v: &str) -> Result<Codec, String> {
    match v { "opus" => Ok(Codec::Opus), "flac" => Ok(Codec::Flac),
              "lossless" => Ok(Codec::Lossless), "lossless-int16" => Ok(Codec::LosslessInt16),
              _ => Err(format!("Onbekende codec: {}", v)) }
}
fn parse_channels(v: &str) -> Result<Channels, String> {
    match v { "mono" => Ok(Channels::Mono), "stereo" => Ok(Channels::Stereo),
              _ => Err(format!("Onbekende channels: {}", v)) }
}
fn parse_filter(v: &str) -> Result<FilterPreset, String> {
    match v { "off" => Ok(FilterPreset::Off), "wide" => Ok(FilterPreset::Wide),
              "voice" => Ok(FilterPreset::Voice), "ssb" => Ok(FilterPreset::Ssb),
              "narrow" => Ok(FilterPreset::Narrow),
              _ => Err(format!("Onbekend filter: {}", v)) }
}

/// Eenvoudige shell-style tokenizer met quoted strings.
fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote: Option<char> = None;
    for c in line.chars() {
        match (in_quote, c) {
            (Some(q), c) if c == q => { in_quote = None; }
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => { in_quote = Some(c); }
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

/// Case-insensitive prefix strip (voor "SET ", "Set ", "set ").
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() { return None; }
    if s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_client() {
        let mut s = BridgeSettings::default();
        s.role = BridgeRole::Client;
        s.codec = Codec::Flac;
        s.channels = Channels::Stereo;
        s.rx_filter = FilterPreset::Ssb;
        s.bitrate = 32000;
        let cmd = build_cmd_script(&s);
        let parsed = parse_script(&cmd).expect("parse roundtrip");
        assert_eq!(parsed.role, BridgeRole::Client);
        assert_eq!(parsed.codec, Codec::Flac);
        assert_eq!(parsed.channels, Channels::Stereo);
        assert_eq!(parsed.rx_filter, FilterPreset::Ssb);
        assert_eq!(parsed.bitrate, 32000);
    }

    #[test]
    fn roundtrip_server() {
        let mut s = BridgeSettings::default();
        s.role = BridgeRole::Server;
        s.iq_swap = IqSwap::Swap;
        s.passthrough = true;
        s.upstream = "ws://192.168.1.10:40001".into();
        let sh = build_sh_script(&s);
        let parsed = parse_script(&sh).expect("parse roundtrip sh");
        assert_eq!(parsed.role, BridgeRole::Server);
        assert_eq!(parsed.iq_swap, IqSwap::Swap);
        assert!(parsed.passthrough);
        assert_eq!(parsed.upstream, "ws://192.168.1.10:40001");
    }

    #[test]
    fn parses_cmd_with_set_vars() {
        let script = "@echo off\r\n\
            set SERVER=ws://test:9001\r\n\
            set LISTEN=127.0.0.1:40002\r\n\
            tci-streamer-client.exe --server %SERVER% --listen %LISTEN% \
            --iq-mode spectrum --codec opus --sample-rate 48000 \
            --frame-ms 20 --bitrate 24000 --channels mono \
            --spectrum-bins 2048 --spectrum-fps 20 \
            --rx-filter ssb --tx-filter off\r\npause\r\n";
        let s = parse_script(script).expect("parse");
        assert_eq!(s.role, BridgeRole::Client);
        assert_eq!(s.server, "ws://test:9001");
        assert_eq!(s.client_listen, "127.0.0.1:40002");
        assert_eq!(s.rx_filter, FilterPreset::Ssb);
    }

    #[test]
    fn parses_sh_with_caret_to_backslash_continuations() {
        // .sh met backslash continuations
        let script = "#!/usr/bin/env bash\n\
            SERVER=ws://test:9001\n\
            ./tci-streamer-client --server $SERVER \\\n\
            --listen 127.0.0.1:40002 --iq-mode spectrum --codec flac \\\n\
            --sample-rate 48000 --frame-ms 20 --bitrate 24000 --channels mono \\\n\
            --spectrum-bins 2048 --spectrum-fps 20 \\\n\
            --rx-filter off --tx-filter off\n";
        let s = parse_script(script).expect("parse sh");
        assert_eq!(s.role, BridgeRole::Client);
        assert_eq!(s.server, "ws://test:9001");
        assert_eq!(s.codec, Codec::Flac);
    }
}
