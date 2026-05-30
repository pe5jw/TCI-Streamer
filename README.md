# tci-streamer

[![CI](https://github.com/USER/tci-streamer/actions/workflows/ci.yml/badge.svg)](https://github.com/USER/tci-streamer/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)

> **Versie 0.1.23** — TCI-proxy met bandbreedte-compressie voor remote ham radio.

TCI-proxy die een Thetis/Zeus radio remote bereikbaar maakt via een smalle
internet-verbinding (VPN/Tailscale). Bestaat uit vijf binaries: een server
op de radio-PC, een client op de bedien-PC, een grafische launcher, een
grafische spectrum/waterfall viewer, en een fake TCI-server voor tests
zonder echte radio.

**Snel beginnen:** Windows? Run [`build.cmd`](#op-windows). Linux/Pi?
Run [`./setup-libopus-linux.sh`](#op-linuxraspberry-pi) en `cargo build --release`.

> ℹ Vervang `USER` in de badges hierboven door je GitHub-username na het forken.

## Architectuur

```
[Thetis/Zeus]──TCI──→[tci-streamer-server]══gecomprimeerde tunnel═══→[tci-streamer-client]──TCI──→[THRA / N1MM / JTDX]
   (LAN)                  (radio-PC)              (VPN/internet)            (remote PC)            (lokaal)
```

## Bandbreedte-besparing

| Stream | Origineel | Spectrum mode | DecimatedIq mode |
|---|---|---|---|
| TCI commando's | <1 kbit/s | <1 kbit/s | <1 kbit/s |
| RX audio (48kHz stereo) | ~3 Mbit/s | 32–96 kbit/s | 32–96 kbit/s |
| IQ (192kHz stereo) | ~12 Mbit/s | ~40 kbit/s | ~3 Mbit/s |
| **Totaal (Spectrum + Balanced)** | **~15 Mbit/s** | **~110 kbit/s** | **~3.1 Mbit/s** |

## Build - quick start

### Op Windows:

```cmd
build.cmd
```

Dat is alles. Het script:

1. Bouwt libopus 1.5.2 (alleen eerste keer, ~2 min)
2. Doet `cargo build --release`
3. Toont waar de binaries staan

Andere modes:
```cmd
build.cmd debug    REM sneller compileren
build.cmd clean    REM opruimen
```

### Op Linux:

```bash
chmod +x build.sh
./build.sh
```

Idem opties: `./build.sh debug`, `./build.sh clean`.

## Vereisten

### Windows
- Rust 1.75 of nieuwer (van [rustup.rs](https://rustup.rs/))
- Visual Studio 2019/2022 Build Tools (gratis) met "C++ CMake tools"
- PowerShell (komt mee met Windows)

### Linux
- Rust 1.75 of nieuwer (van [rustup.rs](https://rustup.rs/) of via je distro)
- `sudo` rechten voor het installeren van `libopus-dev`

## Handmatige build (zonder build script)

### Windows:
```powershell
pwsh -ExecutionPolicy Bypass -File setup-libopus-windows.ps1
cargo build --release
```

### Linux:
```bash
./setup-libopus-linux.sh
cargo build --release
```

## Gebruik

### Op de radio-PC (waar Thetis/Zeus draait):

```bash
tci-streamer-server --upstream ws://127.0.0.1:40001 --listen 0.0.0.0:9001
```

### Op de remote PC:

```bash
tci-streamer-client \
  --server ws://radio.thuis.vpn:9001 \
  --listen 127.0.0.1:40001 \
  --iq-mode spectrum \
  --codec opus --sample-rate 48000 --frame-ms 20 --bitrate 24000 --channels mono \
  --rx-filter ssb --tx-filter ssb
```

Daarna start je THRA, N1MM, JTDX, etc. en wijst die naar `ws://127.0.0.1:40001`.

Versie en help opvragen:
```
tci-streamer-server --version
tci-streamer-server --help
```

## Meerdere instances

Voor twee onafhankelijke koppelingen (bijv. twee Thetissen):

**Radio-PC:**
```bash
tci-streamer-server --upstream ws://127.0.0.1:40001 --listen 0.0.0.0:9001 &
tci-streamer-server --upstream ws://127.0.0.1:40002 --listen 0.0.0.0:9002 &
```

**Remote PC:**
```bash
tci-streamer-client --server ws://radio:9001 --listen 127.0.0.1:40001 &
tci-streamer-client --server ws://radio:9002 --listen 127.0.0.1:40002 &
```

## Modes

### IQ-mode

- `spectrum` (default): server doet de FFT, stuurt alleen power-bins door.
  Maximale compressie. Spectrum/waterfall werkt perfect, maar je verliest
  fase-info.
- `decimated-iq`: 192kHz → 48kHz int16, 16x kleiner dan origineel. Behoudt
  fase-info; bruikbaar voor digital decoders die echte IQ willen.
- `disabled`: geen IQ doorgegeven. Alleen audio + commando's.

### Audio codec (v0.1.23)

In v0.1.23 zijn de vaste profielen vervangen door losse parameters voor
volledige controle over de Opus settings. **Alle audio-instellingen
worden op de CLIENT geconfigureerd** — de server gebruikt automatisch
dezelfde waardes (via het Hello-bericht).

| CLI arg | Default | Toegestaan | Toelichting |
|---------|---------|------------|-------------|
| `--codec` | `opus` | `opus`, `flac`, `lossless`, `lossless-int16` | Opus = lossy, FLAC = lossless ~900kbps, lossless = passthrough |
| `--sample-rate` | `48000` | 8000/12000/16000/24000/48000 | Opus interne rate (lagere = smallere audiobandbreedte) |
| `--frame-ms` | `20` | 2.5/5/10/20/40/60 | Frame duur; 20ms is sweet spot |
| `--bitrate` | `24000` | 6000-510000 | Opus bitrate in bps |
| `--channels` | `mono` | `mono`, `stereo` | Mono halveert de bandbreedte |

Default (`--codec opus --sample-rate 48000 --frame-ms 20 --bitrate 24000
--channels mono`) is gericht op SSB-luisteren: heldere mono spraak bij
~32 kbit/s totale link bandbreedte (incl. Opus headers).

Voor `--bitrate ≤32000` schakelt Opus automatisch naar `Voip` application
mode (spraak-geoptimaliseerd); daarboven naar `Audio` mode (muziek).

#### Voorbeelden

**Optimaal SSB (default, ~32 kbit/s):**
```
--codec opus --sample-rate 48000 --frame-ms 20 --bitrate 24000 --channels mono
```

**Heel zuinig (~16 kbit/s):**
```
--codec opus --sample-rate 16000 --frame-ms 40 --bitrate 16000 --channels mono
```

**Hi-fi stereo (diversity RX, ~115 kbit/s):**
```
--codec opus --sample-rate 48000 --frame-ms 20 --bitrate 96000 --channels stereo
```

**Lage latency QSO (~42 kbit/s):**
```
--codec opus --sample-rate 48000 --frame-ms 10 --bitrate 32000 --channels mono
```

**FLAC lossless mono (~450-700 kbit/s):**
```
--codec flac --channels mono
```

**FLAC lossless stereo (~900 kbit/s - 1.3 Mbit/s):**
```
--codec flac --channels stereo
```

**Diagnose (geen Opus, ~3 Mbit/s):**
```
--codec lossless
```

### Audio bandpass filter

Optioneel HP+LP filter, draait op **zowel server als client** (cascade
geeft steilere flanken). Cutoffs worden op de client ingesteld en
gesynchroniseerd naar de server via Hello.

- `off` (default): geen filter
- `wide`: 100-6000 Hz — breed voor muziek/data
- `voice`: 100-3000 Hz — algemene spraak
- `ssb`: 150-2800 Hz — klassieke amateur SSB
- `narrow`: 200-2800 Hz — smal voor zwakke signalen

**CLI args (alleen op de client):**
- `--rx-filter <preset>` (RX = van radio naar koptelefoon)
- `--tx-filter <preset>` (TX = van microfoon naar radio)

**Plaatsing in het signaalpad:**
- Server RX filter: **voor** Opus encoding (helpt Opus bits besparen)
- Client RX filter: **na** Opus decoding, voor doorgeven aan THRA
- Client TX filter: **voor** Opus encoding (op THRA-microfoon-audio)
- Server TX filter: **na** Opus decoding, voor doorgeven aan Thetis

Het filter cascadeert dus automatisch: een signaal passeert beide filters
in beide richtingen. Bij `--rx-filter ssb --tx-filter ssb` krijg je
150-2800 Hz met 4-pole rolloff in beide richtingen.

## Status v0.1.23

- ✅ TCI text commando's bidirectioneel
- ✅ RX audio Opus-gecomprimeerd (volledig instelbaar)
- ✅ TX audio Opus-gecomprimeerd
- ✅ Audio bandpass filter (server + client cascade)
- ✅ FLAC lossless codec
- ✅ Lossless/lossless-int16 modes voor diagnose
- ✅ TCP_NODELAY op alle sockets (lagere latency)
- ✅ IQ → spectrum FFT compressie
- ✅ IQ → gedecimeerde int16 compressie
- ✅ IQ-swap (Zeus/Thetis conventie: --iq-swap swap|conj)
- ✅ Synthetische IQ reconstructie op client (voor THRA waterfall)
- ✅ Test-tools: fake-tci-server + tci-viewer (end-to-end testen zonder radio)
- ✅ Grafische launcher (tci-launcher): één GUI om server/client te starten
- ✅ Multi-instance via verschillende poorten
- ✅ Auto-reconnect bij verbroken server-link
- ✅ One-shot build scripts (build.cmd / build.sh)
- ✅ Versienummer in CLI (`--version`) en startup-banner
- 🚧 TLS support (v0.2 - gebruik VPN voor nu)

## Veiligheid

Deze tool heeft **geen** ingebouwde authenticatie. Tunnel **altijd**
via VPN (WireGuard, Tailscale, ZeroTier, OpenVPN, etc.).

### TLS / wss:// support

Standaard build heeft geen TLS — dat scheelt dependencies en aansluiting bij
het VPN-advies. Gebruik daarom `ws://` (zonder `s`) in je server URL:

```
tci-streamer-client --server ws://radio.thuis.vpn:9001
```

Wil je toch `wss://` gebruiken (bijvoorbeeld via een reverse proxy als
nginx of Caddy met Let's Encrypt), compileer dan met de `tls` feature:

```
cargo build --release --features tls
```

Op Windows wordt schannel gebruikt (geen OpenSSL nodig). Op Linux is
`libssl-dev` nodig:

```
sudo apt install libssl-dev
```

## Troubleshooting

### "Compatibility with CMake < 3.5 has been removed"

Run `build.cmd` of `setup-libopus-windows.ps1` opnieuw. Het bouwt libopus met
de juiste `-DCMAKE_POLICY_VERSION_MINIMUM=3.5` flag die dit oplost.

### "could not find native library `opus`"

Het setup-script is niet uitgevoerd, of `.cargo/config.toml` is verwijderd.
Run `build.cmd` of `setup-libopus-windows.ps1` opnieuw.

### Op Linux: "pkg-config not found"

```bash
sudo apt install pkg-config        # Debian/Ubuntu
sudo dnf install pkgconfig         # Fedora
```

## Versie

Het versienummer staat in `Cargo.toml` als single source of truth. De binaries
tonen dit bij startup en via `--version`. Bij elke download in deze chat-sessie
wordt het patch-nummer verhoogd (0.1.0 → 0.1.1 → 0.1.2 → 0.1.3 → ...).

## Licentie

tci-streamer staat onder de **MIT License** — zie [`LICENSE`](LICENSE) voor
de volledige tekst.

Alle gebruikte third-party componenten staan onder permissieve licenties
(MIT, Apache-2.0 of BSD). Er zit geen GPL/LGPL/AGPL-besmette code in dit
project — zie [`THIRD-PARTY-LICENSES`](THIRD-PARTY-LICENSES) voor het
volledige overzicht.

Het TCI-protocol zelf is een open specificatie van Expert Electronics; deze
codebase bevat een onafhankelijke implementatie zonder overgenomen code.
"# TCI-Streamer" 
