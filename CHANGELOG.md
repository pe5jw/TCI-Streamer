# Changelog

Alle relevante wijzigingen aan tci-streamer staan hier per versie.

## [0.1.23] - 2026-05-29

### Voorbereiding voor open-source publicatie op GitHub

Project klaargemaakt voor publicatie:
- `LICENSE` (MIT) toegevoegd
- `THIRD-PARTY-LICENSES` met overzicht van alle deps
- `CONTRIBUTING.md` met dev-setup en PR-richtlijnen
- `.gitignore` uitgebreid (target, vendor, IDE-rommel, gegenereerde scripts)
- `.github/workflows/ci.yml` — CI op Linux + Windows met cargo fmt/clippy/build/test
- `.github/workflows/release.yml` — bouwt op v*.*.* tag automatisch Windows-
  en Linux-bundles en maakt GitHub Release met changelog-extract
- `.github/ISSUE_TEMPLATE/{bug_report,feature_request}.md`
- `.github/PULL_REQUEST_TEMPLATE.md`
- README aangevuld met badges en GitHub-hinten
- `docs/PUBLICATIE.md` — stappenplan voor de eerste GitHub-publicatie

### Versienummer consequent overal zichtbaar

- `fake-tci-server`, `tci-viewer`, `tci-launcher` ondersteunen nu `--version`
  (clap), zoals server/client al deden
- `tci-viewer` toont "tci-viewer v{X}" in de venstertitel én als label bovenin
  het UI-paneel (kleur 160,200,255)
- `tci-launcher` idem met "tci-streamer launcher v{X}"
- Single source of truth blijft `Cargo.toml`; alle binaries lezen via
  `env!("CARGO_PKG_VERSION")`

### Licentie-audit voor open-source publicatie

Alle 21 directe Rust-dependencies zijn permissief (MIT / Apache-2.0 / BSD /
Unlicense). libopus = BSD-3-Clause. Geen GPL/AGPL/LGPL-besmetting. Het
TCI-protocol is een open specificatie van Expert Electronics; geen code
overgenomen.

- `LICENSE` (MIT) toegevoegd
- `THIRD-PARTY-LICENSES` met overzicht van alle deps toegevoegd
- README aangevuld met een licentie-sectie

### Auto-start TCI-streams (fixed: viewer direct op Thetis/Zeus werkt nu)

**Probleem.** De tci-viewer werkte op fake-tci-server en via tci-streamer
client/server, maar **niet** direct op een echte Thetis/Zeus TCI-server. Ook
het pad `Zeus → tci-streamer → tci-viewer` (zonder THRA) leverde geen data.

**Oorzaak.** De fake-server start zijn streams uit zichzelf. Een echte
Thetis/Zeus volgt het TCI-protocol netjes en blijft stil totdat de client
expliciet `audio_start;`, `iq_start;` en `start;` stuurt. De viewer deed dat
niet, en de tci-streamer-server forwardt alleen wat THRA stuurt — zonder THRA
ontbreken die commando's.

**Fix in twee delen:**

1. **tci-viewer** stuurt standaard een init-sequentie bij verbinden:
   ```
   audio_samplerate:48000;  iq_samplerate:48000;
   rx_enable:0,true;
   audio_start:0;  iq_start:0;  start;
   ```
   Uitschakelen met `--no-auto-start` (handig als je verbindt met
   tci-streamer client of fake-server die uit zichzelf streamt).

2. **tci-streamer-server** krijgt een `--auto-start` flag (default UIT).
   Aanzetten als je wilt streamen naar alleen tci-viewer (zonder THRA).
   Uitlaten als THRA al de regie heeft over de stream-init.

In de tci-launcher GUI verschijnt nu in het Server-paneel een checkbox
"Auto-start streams" met dezelfde uitleg via tooltip.

### Use-cases

| Scenario | Auto-start nodig? |
|----------|-------------------|
| `Zeus → viewer` direct | viewer: ja (default aan) |
| `Zeus → tci-streamer → viewer` (zonder THRA) | server: ja (`--auto-start`) |
| `Zeus → tci-streamer → THRA` (normaal gebruik) | server: nee (THRA regelt) |
| `fake-server → viewer` (test) | viewer: maakt niet uit, fake-server streamt sowieso |

## [0.1.22] - 2026-05-29

### tci-launcher: Linux .sh scripts + script inlezen

De grafische launcher kan nu start-scripts in beide formaten exporteren én
eerder opgeslagen scripts terug inlezen.

**Save-knoppen** (in het Command-line paneel):
  - **Sla op…** — opent een save-dialog met filter voor `.cmd` (Windows) en
    `.sh` (Linux). De gekozen extensie bepaalt het formaat. Op Linux krijgt
    een `.sh` automatisch executable-bit (chmod 755).
  - **Sla beide op…** — vraagt om een map en schrijft daarin zowel
    `start-{server,client}.cmd` als `.sh`. Handig voor projecten die op
    meerdere platforms gedeployd worden.
  - **Laad…** — leest een `.cmd` of `.sh` script in en vult alle GUI-velden
    met de gevonden waarden. Variabelen (`%NAME%` in cmd, `$NAME`/`${NAME}`
    in sh) worden geëxpandeerd. Line-continuations (`^` resp. `\`) worden
    netjes samengevoegd.

Hiermee kun je een script eenmalig opslaan, later weer openen in de launcher,
de instellingen tweaken, en opnieuw exporteren — handig voor het beheren van
meerdere configuraties.

### Linux .sh format

```sh
#!/usr/bin/env bash
# Gegenereerd door tci-streamer-launcher
set -e
cd "$(dirname "$0")"
./tci-streamer-client --server ws://... --listen 127.0.0.1:40002 ...
```

Geen `pause` aan het eind (anders dan .cmd), executable-bit gezet, gebruikt
de Linux-binary-naam zonder `.exe`.

## [0.1.21] - 2026-05-28

### tci-streamer-launcher: GUI-launcher in Rust (vervangt C# versie)

De C# WinForms-launcher (`tci-streamer-gui`, los project) is herschreven in
Rust met eframe, zodat **alles met één `cargo build`** in één pakket komt.

Functionaliteit identiek aan de C# versie:
  - Server/Client mode toggle
  - Alle CLI-opties als dropdowns/velden (incl. `--iq-swap`)
  - Live command-line preview
  - Start/Stop met process spawning + log venster
  - "Sla .cmd op" voor headless gebruik
  - Folder picker voor de programma-map

```cmd
target\release\tci-launcher.exe
```

### Voordelen één-pakket-aanpak

- **Eén build-stap**: `cargo build --release` levert alle vijf de binaries
- **Eén toolchain**: alleen Rust nodig (geen .NET SDK meer)
- **Cross-platform**: launcher draait nu ook op Linux/Pi (mits display)
- **Geen taal-grens** tussen launcher en streamer-code: gedeelde
  `BridgeSettings` en `build_args` in `src/launcher/mod.rs`

De C# versie blijft als referentie bestaan in het losse `tci-streamer-gui`
project, maar wordt niet meer onderhouden.

### Build-output

`build.cmd` en `build.sh` tonen nu vijf binaries:
  - `tci-streamer-server`
  - `tci-streamer-client`
  - `fake-tci-server`
  - `tci-viewer`
  - `tci-launcher`  ← nieuw

Headless builds (zonder GUI's) blijven mogelijk:
```bash
cargo build --release --no-default-features
```
levert alleen de drie streaming-binaries.

## [0.1.20] - 2026-05-28

### tci-viewer is nu een grafische applicatie (eframe/egui)

De terminal-only viewer uit v0.1.23 is vervangen door een echte GUI-applicatie
met `eframe`/`egui`. Toont in één venster:

  - **IQ-spectrum**: live FFT-plot van de IQ-stream, dB-schaal met gridlijnen
  - **Waterfall**: scrollende kleurgecodeerde 2D weergave (200 rijen historie),
    nieuwste bovenaan
  - **Audio-spectrum**: FFT van de RX-audio
  - **Audio-VU**: kleurgecodeerde niveau-balk (groen/geel/rood)

Bovenaan zitten sliders voor het dB-bereik (min/max) zodat je de
spectrum-/waterfall-contrast kunt afstemmen op je signaal.

```cmd
REM toon wat er uit tci-streamer client komt:
tci-viewer.exe --connect ws://127.0.0.1:40002

REM of direct de fake server / Thetis:
tci-viewer.exe --connect ws://127.0.0.1:40001 --iq-fft 2048 --waterfall-rows 300
```

### Feature flag voor eframe

eframe is een forse dependency (~50 extra crates). Daarom zit hij achter de
`viewer` feature flag, standaard aan. Wil je sneller bouwen zonder de viewer:

```bash
cargo build --release --no-default-features
```

Dan worden alleen `tci-streamer-server`, `tci-streamer-client` en
`fake-tci-server` gebouwd.

### Linux setup uitgebreid

`setup-libopus-linux.sh` installeert nu ook de eframe systeem-deps
(`libgl1-mesa-dev`, `libxkbcommon-dev`) zodat de GUI direct werkt. Op een
Raspberry Pi werkt het zo ook, mits de Pi grafische output heeft (X11 of
Wayland). Op een headless Pi zonder display: bouw met
`--no-default-features` om de viewer over te slaan.

## [0.1.19] - 2026-05-28

### Toegevoegd: IQ-swap (Zeus/Thetis conventie)

Nieuwe server-optie `--iq-swap <none|swap|conj>`. Het verschil tussen een
Zeus- en een Thetis-server zit vaak in de IQ-conventie, waardoor de waterfall
gespiegeld kan zijn. Met deze optie corrigeer je dat:

  - `none`: geen wijziging (default)
  - `swap`: verwissel I en Q per sample → spiegelt het spectrum
  - `conj`: keer het teken van Q om (complex conjugaat) → spiegelt rond DC

De swap wordt toegepast vóór FFT/decimatie, dus de spectrum-output (en de
synthetische IQ-reconstructie op de client voor THRA's waterfall) is meteen
correct georiënteerd. Probeer `swap` als de waterfall gespiegeld is; `conj`
als alternatief.

```cmd
tci-streamer-server.exe --upstream ws://127.0.0.1:40001 --listen 0.0.0.0:9001 ^
    --iq-swap swap
```

### Toegevoegd: test-tools (twee nieuwe binaries)

**`fake-tci-server`** — emuleert een Thetis/Zeus TCI-server zodat je
tci-streamer kunt testen zonder echte radio-software. Luistert als
WebSocket-server, stuurt een audio-testtoon (1 kHz default) en een IQ-stream
met herkenbare, asymmetrische spectrum-pieken (zodat IQ-swap zichtbaar wordt),
en beantwoordt TCI-commando's.

```cmd
fake-tci-server.exe --listen 127.0.0.1:40001
REM dan: tci-streamer-server --upstream ws://127.0.0.1:40001 --listen 0.0.0.0:9001
```

**`tci-viewer`** — verbindt met een TCI WebSocket stream en toont live in de
terminal het audio-niveau (VU-meter) en het IQ-spectrum (ASCII-weergave).
Handig om te zien of audio en spectrum doorkomen, zonder THRA.

```cmd
REM toon wat er uit de tci-streamer client komt:
tci-viewer.exe --connect ws://127.0.0.1:40002
REM of direct de fake server / Thetis bekijken:
tci-viewer.exe --connect ws://127.0.0.1:40001
```

### Test-keten (volledig zonder radio/THRA)

```
fake-tci-server  ──ws://…:40001──>  tci-streamer-server  ──ws://…:9001──>  tci-streamer-client  ──ws://…:40002──>  tci-viewer
   (emuleert Thetis)                   (comprimeert)                          (decomprimeert)                         (toont audio+spectrum)
```

Hiermee kun je de hele pijplijn end-to-end testen op één PC.

## [0.1.18] - 2026-05-28

### Verwijderd: jitter-buffer (was de bron van de ratel!)

De jitter-buffer uit v0.1.17 (`--jitter-buffer`) is volledig verwijderd.
Envelope-spectrum-analyse van opnames over de schone verbinding toonde
onomstotelijk aan dat de jitter-buffer ZELF de ratel veroorzaakte:

| Instelling | 23,4 Hz harmonics | Oordeel |
|------------|-------------------|---------|
| `--jitter-buffer 0` (uit) | 0/6 sterk | schoon |
| `--jitter-buffer 150` | 2/6 sterk | ratel |

De oorzaak: de jitter-buffer leverde frames uit op een vaste software-timer
(2048 samples = 42,667 ms = 23,4375 Hz). Die timer liep niet synchroon met
de werkelijke audio-instroom (Thetis' sample-klok via de codec). De twee
klokken driftten uit elkaar → periodieke onderloop → een click op elke tik
→ exact de 23,4 Hz harmonische reeks die we als "ratel" hoorden.

RadioBridge had dit probleem niet omdat dáár de geluidskaart-hardware de
klok bepaalt en de afspeelbuffer continu uit dezelfde bron leest. Een losse
software-timer als klok was de ontwerpfout.

Met het onderliggende MTU-probleem opgelost (zie hieronder) is directe
doorgifte van audio schoon — geen jitter-buffer nodig.

### Behouden: TCP_NODELAY

`TCP_NODELAY` staat nu op alle netwerk-sockets (client lokale listener,
client→server verbinding, server accept). Verlaagt latency en voorkomt dat
kleine TCI-commando's/audioframes worden opgespaard door Nagle. De
client→server verbinding maakt voor `ws://` nu zelf de TCP-socket aan om
nodelay te kunnen zetten; `wss://` gebruikt nog de standaard route.

### Context: het MTU/Tailscale-probleem (netwerk, geen code)

Het "onwerkbare" deel van de ratel over de remote-verbinding bleek een
MTU-probleem: een Tailscale subnet-router met een PMTU-black-hole liet
grote pakketten (waaronder de WebSocket-handshake) stil verdwijnen.
Opgelost op de router met een vaste TCP MSS-clamp (set-mss 1240) op de
tailscale-interface. Dit is netwerk-configuratie, geen tci-streamer-wijziging,
maar wordt hier vermeld omdat het cruciaal was voor een werkende verbinding.

## [0.1.17] - 2026-05-21

### Toegevoegd: jitter-buffer voor RX audio (opt-in)

Nieuwe `--jitter-buffer <ms>` optie op de client. Standaard 0 (uit).

**Achtergrond.** Onderzoek naar RadioBridge (een andere remote-audio tool die
soepel klinkt) wees uit dat het verschil niet in de codec zit maar in de
afspeel-keten: RadioBridge heeft een echte jitter-buffer (NAudio's
`BufferedWaveProvider`) waar de geluidskaart op een vaste klok uit trekt.
tci-streamer had dat niet — het stuurde elke gedecodeerde frame direct door op
netwerk-timing, waardoor de TCI-client (THRA, N1MM, JTDX, ...) ongelijkmatig
getimede frames kreeg → mogelijke ratel.

Een buffer-fix in THRA zou alleen THRA helpen; andere TCI-clients blijven dan
last houden. Daarom zit de jitter-buffer in tci-streamer zelf, vóór de
uitlevering — zo profiteert elke TCI-client.

**Werking.**
- Gedecodeerde RX audio (Opus/FLAC/lossless) gaat in een ringbuffer.
- Een timer trekt op vast tempo (2048 samples @ 48kHz = 42,667 ms) precies één
  frame en stuurt dat als TCI binary naar de client. Dit ontkoppelt
  ontvangst-timing (netwerk, jitter) van uitlever-timing (vaste klok) — het
  software-equivalent van RadioBridge's geluidskaart-klok.
- **Pre-fill:** levert pas audio uit zodra de buffer voor het eerst de
  ingestelde diepte bereikt; daarvoor stilte.
- **Onderloop:** vult stilte aan en herbegint pre-fill (voorkomt frame-voor-frame
  ratelen bij aanhoudend tekort).
- **Overloop:** dropt oudste samples (max 3× de target-diepte), zodat latency
  niet oploopt — net als RadioBridge's `DiscardOnBufferOverflow`.

**Gebruik.**
```cmd
REM Zonder jitter-buffer (huidige gedrag, voor A/B vergelijking):
tci-streamer-client.exe --server ws://192.168.8.141:9001 --listen 127.0.0.1:40002

REM Met jitter-buffer, 100ms diep:
tci-streamer-client.exe --server ws://192.168.8.141:9001 --listen 127.0.0.1:40002 ^
    --jitter-buffer 100
```

Typische waarden: 80-150 ms. Hoger = meer jitter-tolerantie maar meer latency.
De buffer voegt z'n diepte als extra latency toe.

### Test-aanpak

Draai eerst zonder `--jitter-buffer` (baseline ratel), dan met
`--jitter-buffer 100`. Als de ratel verdwijnt of duidelijk minder wordt, is de
netwerk-jitter de oorzaak en is dit de juiste fix. Werkt onafhankelijk van de
codec (`--codec opus/flac/lossless`).

## [0.1.16] - 2026-05-21

### Toegevoegd: FLAC lossless codec

Nieuwe `--codec flac` optie. FLAC geeft bit-perfecte (lossless) audio bij
~40-60% van de originele grootte (~900 kbit/s typisch), tussen de lossy
Opus (~24-96 kbit/s) en de raw lossless modes (~1.5-3 Mbit/s) in.

**Eigenschappen:**
- Pure-Rust implementatie: `flacenc` (encode) + `claxon` (decode). Géén
  C-vendoring of CMake nodig — anders dan libopus. Compileert direct op
  Windows MSVC en Linux.
- Elke audio-chunk wordt als compleet zelf-bevattend FLAC-stream verstuurd
  (incl. STREAMINFO header), zodat packet-verlies één chunk treft en niet
  de hele stream desynchroniseert.
- Werkt voor RX én TX, met mono-downmix als `--channels mono`.
- Alleen sample-rate 48000 ondersteund (geen resampling in FLAC-pad).

**Nieuwe frame types:** `FlacRxAudio` (0x14), `FlacTxAudio` (0x15).

### Gebruik

```cmd
tci-streamer-client.exe --server ws://192.168.8.141:9001 ^
    --listen 127.0.0.1:40002 --codec flac --channels mono
```

Stereo FLAC (hogere bandbreedte, behoudt diversity):
```cmd
tci-streamer-client.exe --server ... --codec flac --channels stereo
```

### Wanneer FLAC kiezen

- **Opus** (~24 kbit/s): zuinigst, lossy, prima voor SSB-luisteren
- **FLAC** (~900 kbit/s): lossless, voor wanneer je bit-perfecte audio wilt
  maar niet de volle 1.5-3 Mbit/s van raw lossless. Goede keuze op een
  snelle LAN/VPN link waar bandbreedte geen probleem is maar je geen Opus
  artefacten wilt.
- **lossless / lossless-int16**: diagnose, of als FLAC-encoding te veel CPU
  kost op de server.

### Bandbreedte-overzicht (48kHz, ruis comprimeert slecht, spraak goed)

| Codec | Typisch | Lossless? |
|-------|---------|-----------|
| `opus` 24kbps mono | ~32 kbit/s | nee |
| `opus` 96kbps stereo | ~115 kbit/s | nee |
| `flac` mono | ~450-700 kbit/s | ja |
| `flac` stereo | ~900 kbit/s - 1.3 Mbit/s | ja |
| `lossless-int16` | ~1.5 Mbit/s | ja |
| `lossless` | ~3 Mbit/s | ja |

## [0.1.15] - 2026-05-19

### Major: Audio config volledig herzien

De vaste `--audio` profielen (lossless/lossless-int16/low-bandwidth/balanced/
low-latency) zijn vervangen door losse parameters waarmee je elke Opus
configuratie kunt kiezen.

### Nieuwe CLI args (alleen op de client, server volgt automatisch)

| Arg | Default | Toegestaan |
|-----|---------|------------|
| `--codec` | `opus` | `opus`, `lossless`, `lossless-int16` |
| `--sample-rate` | `48000` | 8000, 12000, 16000, 24000, 48000 (Opus eis) |
| `--frame-ms` | `20` | 2.5, 5, 10, 20, 40, 60 (Opus eis) |
| `--bitrate` | `24000` | 6000-510000 bps |
| `--channels` | `mono` | `mono`, `stereo` |
| `--rx-filter` | `off` | `off`, `wide`, `voice`, `ssb`, `narrow` |
| `--tx-filter` | `off` | idem |

### Server sync via Hello

De server kreeg z'n eigen CLI filter args (`--rx-filter`, `--tx-filter`)
**verwijderd**. Audio codec config én filter cutoffs reizen nu mee in de
Hello-frame van client naar server. Daardoor:

- Eén enkele plek (client CLI) bepaalt alle audio settings
- Server en client gebruiken gegarandeerd dezelfde codec parameters
- Geen mismatch mogelijk tussen codec sample-rate, channels, bitrate

### TX audio pad geïmplementeerd

De TX pijplijn (THRA → client → server → Thetis) was tot v0.1.14 stub.
Nu volledig functioneel:

- Client: ontvangt TCI TxAudioStream van THRA → TX filter → mono downmix
  (indien `--channels mono`) → resample naar codec rate → Opus encode →
  OpusTxAudio frame naar server
- Server: ontvangt OpusTxAudio → Opus decode → resample naar 48kHz →
  upmix naar stereo → TX filter → TCI binary frame naar Thetis

Voor `--codec lossless` en `--codec lossless-int16` gaat de TX audio
zonder Opus door (RawTxAudio frames).

### Codec internals

- `Application::Voip` wordt automatisch gekozen bij bitrate ≤32 kbit/s
  (beter voor spraak), `Application::Audio` daarboven (beter voor muziek)
- Lineaire resampling tussen Thetis' 48 kHz en de codec sample rate
- Stereo→mono mix via L+R gemiddelde

### Voorbeeld gebruik

**SSB-optimaal (24 kbit/s mono, klein filter):**
```cmd
tci-streamer-client.exe --server ws://192.168.8.141:9001 ^
    --listen 127.0.0.1:40002 --codec opus --sample-rate 24000 ^
    --frame-ms 20 --bitrate 24000 --channels mono ^
    --rx-filter ssb --tx-filter ssb
```

**Diagnose (lossless, geen Opus):**
```cmd
tci-streamer-client.exe --server ws://192.168.8.141:9001 ^
    --listen 127.0.0.1:40002 --codec lossless
```

**Stereo high-quality:**
```cmd
tci-streamer-client.exe --server ws://192.168.8.141:9001 ^
    --listen 127.0.0.1:40002 --codec opus --sample-rate 48000 ^
    --frame-ms 20 --bitrate 96000 --channels stereo
```

### Breaking change waarschuwing

Een client van v0.1.23 spreekt **niet** met een server ouder dan v0.1.23
(Hello JSON heeft nieuwe velden). Update altijd beide kanten samen.

## [0.1.14] - 2026-05-11

### Toegevoegd
- **Audio bandpass filter** met vaste presets (biquad IIR, 2-pole Butterworth
  high-pass + 2-pole low-pass cascade). Beschikbaar op zowel server als
  client, voor RX en TX richting onafhankelijk.

**Presets:**
- `off` — geen filter (default)
- `wide` — 100-6000 Hz
- `voice` — 100-3000 Hz
- `ssb` — 150-2800 Hz
- `narrow` — 200-2800 Hz

**Nieuwe CLI args (zowel server als client):**
- `--rx-filter <preset>` — filter voor RX audio
- `--tx-filter <preset>` — filter voor TX audio

**Plaatsing:**
- Server RX: filter draait **voor** Opus encoding (helpt Opus bits besparen)
- Server TX: filter draait **na** Opus decoding (voor doorgeven aan Thetis)
- Client RX: filter draait **na** Opus decoding (voor doorgeven aan THRA)
- Client TX: geconfigureerd; actief zodra TX audio pad geïmplementeerd is

Server en client zijn volledig onafhankelijk. Beide filters cascaden geeft
een steiler effect zonder bandbreedte-impact.

### Voorbeeld gebruik
```cmd
target\release\tci-streamer-server.exe --upstream ws://127.0.0.1:40001 ^
    --listen 0.0.0.0:9001 --rx-filter voice

target\release\tci-streamer-client.exe --server ws://192.168.8.141:9001 ^
    --listen 127.0.0.1:40002 --audio balanced --rx-filter ssb
```

Dit cascadeert server-side `voice` (100-3000 Hz) met client-side `ssb`
(150-2800 Hz), wat in de praktijk neerkomt op ~150-2800 Hz met steilere
flanken dan een enkele preset.

## [0.1.13] - 2026-05-11

### Verwijderd
- **Rebuffer in client.rs (v0.1.12 introductie) is verwijderd.** De rebuffer
  bleek juist de ratel te VEROORZAKEN, niet op te lossen.

### Bevinding via envelope-spectrum analyse
Audio samples van v0.1.12 zijn geanalyseerd met envelope-FFT:
- `direct.wav` (zonder tci-streamer): geen periodieke artefacten
- `lossless.wav` (lossless audio mode): geen 23.4 Hz harmonics
- `balanced.wav` (Opus + rebuffer): **perfecte harmonische reeks bij
  23.4 Hz, 46.8, 70.2, 93.6, 117, 140, 163, 187, 210, 234, 257, 281 Hz**

23.4 Hz = exact 48000 / 2048 — onze rebuffer chunk-grootte uit v0.1.12.
Elke chunk-grens (om de 42.67 ms) introduceerde een hoorbare discontinuïteit.

### Achtergrond
De v0.1.12 theorie was: THRA verwacht ~2048 sample frames, dus knip onze
Opus output (480/960/1920 samples) op in 2048-sample chunks. Wat we niet
beseften: tussen Opus-decoded frames is er een kleine numerieke
discontinuïteit (normaal voor codecs met inter-frame state), maar zolang
THRA elke decoded frame als één compleet TCI binary frame krijgt, smeert
THRA dat uit in zijn eigen audio buffer. Door **midden in** een Opus
frame te knippen op een willekeurige 2048-sample boundary, creëerden we
een artificiële discontinuïteit die anders niet bestond.

### Volgende stap
De ratel die er voor v0.1.12 ook al was (de "kleinere" ratel die de
gebruiker eerder als "iets minder erg" beschreef bij low-latency) is nu
weer aanwezig. Die hebben we nog niet opgelost. Een nieuwe envelope-FFT
test op v0.1.23 zal moeten uitwijzen of die residuele ratel op andere
frequenties zit (bv. 50 Hz = 20ms Opus frame boundary, of 100 Hz =
10ms boundary).

## [0.1.12] - 2026-05-09

### Bevinding uit v0.1.11 tests
De testresultaten van de gebruiker (lossless schoon, balanced ratelig,
low-latency iets minder ratelig) wijzen naar een mismatch tussen onze
Opus-frame output en wat THRA verwacht. Thetis stuurt audio frames van
~2048 stereo-samples (`audio_stream_samples:2048`), maar onze client
geeft elke gedecodeerde Opus output (480/960/1920 samples bij 10/20/40ms
profielen) direct als één TCI binary frame door. Dat zijn 5-10× meer
audio-events per seconde dan THRA verwacht; die hoge event-rate
veroorzaakt waarschijnlijk de ratel.

### Toegevoegd
- **Audio rebuffering op client**: Opus-decoded output wordt nu eerst
  gebufferd tot ~2048 stereo-samples voor het als TCI binary frame naar
  THRA wordt gestuurd. Dit matched Thetis' eigen `audio_stream_samples`
  waarde en zou THRA gelijkmatig moeten voeden zonder kleine-frame
  artefacten. Lossless modes blijven ongewijzigd (geen rebuffering nodig).

## [0.1.11] - 2026-05-09

### Toegevoegd
- Twee nieuwe audio profielen voor diagnose en high-fidelity gebruik:
  - `lossless`: float32 1:1 doorgegeven, ~3 Mbit/s, geen Opus.
  - `lossless-int16`: int16 conversie, ~1.5 Mbit/s, geen lossy compressie.
- Twee nieuwe frame types in het wire protocol: `RawRxAudio` (0x12) en
  `RawTxAudio` (0x13) voor de lossless audio modes.
- Functies `build_raw_audio` en `parse_raw_audio` in `proto`.
- `AudioProfile::uses_opus()` voor server-side routering.

### Achtergrond
De ratel die we eerder probeerden te debuggen is volgens de gebruiker
"aanmerkelijk erger met TCI streamer" dan zonder. Een lichte ratel op SSB
is al aanwezig in de direct-Thetis-naar-THRA flow, maar onze proxy maakt
hem onwerkbaar. Door `lossless` als test-optie toe te voegen kunnen we
chirurgisch bepalen of de Opus pipeline (encode op server, decode op
client, rebuild naar TCI binary) de schuldige is. Als ratel verdwijnt bij
`lossless`, weten we zeker dat de oorzaak in Opus zit. Als ratel ook bij
`lossless` aanwezig is, ligt het probleem ergens anders (bv. in de
frame-rebuild op de client, of in de IQ-stream die THRA's audio-decode
beïnvloedt).

### Test-procedure
```cmd
target\release\tci-streamer-client.exe --server ws://192.168.8.141:9001 ^
    --listen 127.0.0.1:40002 --audio lossless --iq-mode disabled
```
Vergelijk de audio-kwaliteit met de eerdere `balanced`-test. Als `lossless`
schoon klinkt en `balanced` rateligt → Opus pipeline is de bron.

## [0.1.10] - 2026-05-09

### Toegevoegd
- `--log-tci-commands` flag voor de server: logt elk TCI text commando
  (ingaand en uitgaand) zodat we de stream-init flow kunnen volgen.
  Helpt zien welke `iq_start`, `audio_start`, `audio_stream_*` commando's
  Thetis krijgt en wanneer.
- Stream type counter: server logt elke 5 seconden hoeveel frames van
  elk type binnenkwamen (IQ / RxAudio / TxAudio / Anders). Onmiddellijk
  duidelijk of audio frames uberhaupt aankomen.
- Diagnostic log "Eerste RX audio frame: rate=X Hz, samples=Y" zodra
  we daadwerkelijk een type=1 frame zien.

### Aangepast
- `AudioEncoder` accepteert nu de input sample-rate uit het TCI binary
  header en doet lineaire resampling naar 48kHz als die afwijkt. Vermoeden
  was dat hardcoded 48000 problemen geeft als Thetis een andere rate
  stuurt — nog niet bevestigd, log uit v0.1.9 toonde alleen IQ frames.

### Open vragen na v0.1.9 diagnose
- In de log van v0.1.9 zien we GEEN audio frames in de eerste 10 binary
  frames, alleen IQ. Toch is er audio hoorbaar (met ratel). Vraag:
  hoe komt audio bij THRA terecht? Mogelijk via de IQ stream zelf
  (THRA demoduleert IQ) — wat zou betekenen dat de "audio ratel" eigenlijk
  IQ-discontinuiteit is.

## [0.1.9] - 2026-05-09

### Gefixt — DE OORZAAK VAN DE RATEL
- TCI binary header `length` veld werd ten onrechte als sample-count
  geinterpreteerd. Diagnostiek uit v0.1.8 onthulde:
  ```
  totale lengte=1072 bytes -> 260 floats payload
  geparsed: length=252  <- veld in header
  ```
  Het verschil van 8 floats per frame werd systematisch genegeerd, wat
  een vaste-tempo klik veroorzaakte aan elke frame-grens (zichtbaar in
  zowel de audio als het IQ-spectrum).
- `read_f32_samples` en `classify_tci_binary` rekenen nu de echte
  sample-count uit de WebSocket message-grootte: `(totale - 32) / 4`.
- Het `length` header-veld wordt niet meer gebruikt; het bevat blijkbaar
  iets anders (bytes-count van een sub-section, of een reserve-waarde).

### Opgevallen tijdens diagnostiek
- Sample rate parsing klopt: 192000 Hz voor IQ frames. Mijn header
  layout assumpties waren correct op offset 0 (receiver), 4 (rate),
  8 (format=3 voor float32), 24 (stream_type).
- Passthrough mode werkt zonder ratel — bevestigt dat het probleem in
  onze codec-kant zat, niet in de transport-laag of TCI-protocol zelf.

## [0.1.8] - 2026-05-09

### Toegevoegd
- Diagnostic `--passthrough` mode op de server: stuurt TCI binary
  frames 1:1 door naar de client zonder enige codec of FFT. Geen
  bandbreedte-reductie, alleen voor debugging. Helpt onderscheiden
  of de ratel uit onze codec/parser komt of uit iets anders zoals
  WebSocket message-grenzen.
- Diagnostic `--log-first-frames N` op de server: dumpt de eerste
  N binary frames als hex met de geinterpreteerde header velden
  (receiver, sample_rate, format, codec, crc, length, stream_type)
  en payload-statistieken. Helpt om de TCI binary header layout te
  verifieren.
- Nieuw frame-type `RawTciBinary` (0xFE) in het wire-protocol voor
  passthrough doorgifte.

### Bekend probleem
- Klikken met vast tempo zichtbaar in zowel audio als spectrum bij
  een eerste end-to-end test. Diagnostiek toegevoegd om de oorzaak
  te lokaliseren in v0.1.23.

## [0.1.7] - 2026-05-09

### Toegevoegd
- Eerste succesvolle build van Rust binaries! `tci-streamer-server.exe`
  en `tci-streamer-client.exe` worden nu gegenereerd in `target\release\`.
- Optionele TLS feature voor `wss://` connecties:
  ```
  cargo build --release --features tls
  ```
  Gebruikt schannel op Windows (geen OpenSSL nodig) en native-tls op
  Linux. Standaard uit, want VPN tunneling is de aanbevolen aanpak.

### Gefixt
- Heldere foutmelding wanneer client `wss://` URL krijgt maar zonder
  TLS feature gecompileerd is. Geen mysterieuze "TLS support not
  compiled in" reconnect-loop meer; in plaats daarvan een directe
  uitleg met de twee oplossingen (ws:// gebruiken of hercompileren).

### Aangepast
- `tokio-tungstenite` dependency op `default-features = false` met
  alleen `connect` en `handshake` aan, voor minimale builds.
- README heeft nu een aparte "TLS / wss:// support" sectie.

## [0.1.6] - 2026-05-09

### Gefixt
- Compile-fout in `src/codec/iq.rs`: literal `-140` als `i8` (range
  -128..127). Het dB-bereik is teruggebracht naar -128..0 dBFS — nog
  steeds 128 dB dynamisch bereik wat ruim is voor radio waar de
  ruisvloer typisch tussen -100 en -120 dBFS ligt.
- `(db_max - db_min) as f32` arithmetic kon op runtime overflowen
  (resultaat 128 past niet in i8). Nu via `i32` arithmetic op twee
  plekken: `SpectrumProcessor::build_frame` en `synth_iq_from_spectrum`.
- Ongebruikte `use anyhow::Result` import in `iq.rs` weggehaald.

### Bekend
- Eerste succesvolle build van de Rust binaries (`tci-streamer-server`
  en `tci-streamer-client`) wordt verwacht in deze versie. Alle
  build-systeem issues (CMake policies, libopus linker paths, BOM/CRLF,
  PowerShell locale parsing) zijn in eerdere versies opgelost.

## [0.1.5] - 2026-05-09

### Gefixt
- Linker kon `opus.lib` niet vinden ("could not find native static library
  `opus`, perhaps an -L flag is missing?"). De `OPUS_LIB_DIR` env var
  alleen is niet voldoende; de Rust linker heeft expliciet een
  `-L native=...` flag nodig.
- `.cargo/config.toml` bevat nu ook `rustflags = ["-L", "native=..."]`
  voor de drie Windows targets (x64/x86/ARM64) zodat de linker libopus
  vindt op de exacte locatie waar het PS1 script hem heeft neergezet.
- `Write-CargoConfig` is nu een PowerShell functie die altijd wordt
  aangeroepen, ook als libopus al gebouwd is. Zo krijgen upgrades van
  het script automatisch de nieuwste configuratie zonder dat libopus
  opnieuw gebouwd hoeft te worden.

### Aangepast
- `build.cmd` roept de PS1 nu altijd aan in plaats van een eigen check
  te doen op `vendor\opus\lib\opus.lib`. Dat is OK want PS1 doet zelf
  een snelle skip als libopus al bestaat (alleen de config wordt dan
  herschreven).

## [0.1.4] - 2026-05-09

### Gefixt
- CMake parameter parsing in `setup-libopus-windows.ps1`: PowerShell
  splitste de waarde "3.5" van `-DCMAKE_POLICY_VERSION_MINIMUM=3.5` op
  in "3" en ".5" omdat het een Nederlandse locale gebruikt en de punt
  als decimaal-scheidingsteken in een nummer-context interpreteert.
  Opgelost door alle CMake argumenten als string-array te bouwen en
  via splatting (`@CMakeArgs`) door te geven.
- UTF-8 BOM verwijderd uit `build.cmd`. CMD.exe begrijpt geen BOM en
  toonde daardoor "'@echo' is not recognized" bij de eerste regel,
  waardoor `@echo off` niet werkte en alle commando's getoond werden.
  BOM blijft wel in `.ps1` voor PowerShell 5 compatibiliteit.

## [0.1.3] - 2026-05-09

### Gefixt
- CMake auto-detectie in `setup-libopus-windows.ps1`. CMake wordt nu
  automatisch gevonden via meerdere strategieen op volgorde:
  1. PATH (`where cmake`)
  2. `vswhere.exe` voor alle Visual Studio installaties
  3. Glob over `C:\Program Files\Microsoft Visual Studio\*\*\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe`
  4. Standalone CMake locaties (`C:\Program Files\CMake`, Chocolatey)
- Heldere foutmelding met installatie-instructies als CMake niet
  gevonden kan worden.
- CMake wordt aangeroepen via volledig pad, dus PATH-aanpassingen zijn
  niet meer nodig.

## [0.1.2] - 2026-05-09

### Gefixt
- PowerShell parse-fout in `setup-libopus-windows.ps1` veroorzaakt door
  em-dash karakters (U+2014) die door Windows PowerShell 5 verkeerd
  geinterpreteerd werden als Windows-1252. Alle non-ASCII karakters in
  shell-scripts zijn vervangen door ASCII equivalenten.
- UTF-8 BOM toegevoegd aan `.ps1` en `.cmd` bestanden zodat Windows
  PowerShell 5 ze altijd correct als UTF-8 leest.
- CRLF line-endings voor `.ps1` en `.cmd` bestanden voor maximale
  Windows compatibiliteit.

## [0.1.1] - 2026-05-09

### Toegevoegd
- `build.cmd` voor Windows: een-commando build die libopus setup en
  cargo build automatisch achter elkaar doet.
- `build.sh` voor Linux: equivalent voor Linux systemen.
- Versienummer in CLI: `--version` en startup-banner tonen het
  versienummer uit Cargo.toml.
- CHANGELOG.md voor het bijhouden van wijzigingen.

### Aangepast
- README is herschreven met de quick-start build commando's vooraan.
- Setup-scripts tonen nu het versienummer in de banner.

## [0.1.0] - 2026-05-08

Initial proof-of-concept release.

- Eerste werkende skeleton met server/client architectuur.
- Wire protocol met Hello-handshake voor mode-onderhandeling.
- TCI text commando's bidirectioneel.
- RX audio Opus-gecomprimeerd (3 profielen: 32/64/96 kbit/s).
- IQ -> spectrum FFT compressie (~40 kbit/s).
- IQ -> gedecimeerde int16 compressie (~3 Mbit/s).
- Synthetische IQ reconstructie op client.
- Multi-instance via verschillende poorten.
- Auto-reconnect bij verbroken server-link.
- libopus integratie via vendored build die CMake-policy issues
  met moderne CMake 4.0+ omzeilt.
