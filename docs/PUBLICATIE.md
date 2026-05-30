# Stappenplan: tci-streamer publiceren op GitHub

Deze gids leidt je door het publiceren van tci-streamer als open-source
project op GitHub. Tijd: ~30 minuten voor de eerste publicatie.

---

## Stap 0 — Voorbereiding (eenmalig)

### 0.1 GitHub-account
Als je er nog geen hebt: maak een account op https://github.com/. Een
**publiek** persoonlijk account is genoeg.

### 0.2 Git lokaal
Check of git geïnstalleerd is:
```cmd
git --version
```
Zo niet: download van https://git-scm.com/ (Windows) of `sudo apt install git`
(Linux).

### 0.3 Git configureren
Eenmalig:
```cmd
git config --global user.name "Joeri ..."
git config --global user.email "jouw-email@example.com"
git config --global init.defaultBranch main
```

### 0.4 SSH-key voor GitHub (aanbevolen)
Maakt push/pull authenticatie pijnloos. Eenmalig:
```cmd
ssh-keygen -t ed25519 -C "jouw-email@example.com"
```
Plak de inhoud van `~/.ssh/id_ed25519.pub` (Linux) of
`%USERPROFILE%\.ssh\id_ed25519.pub` (Windows) in GitHub → Settings → SSH and
GPG keys → New SSH key.

---

## Stap 1 — Pas LICENSE aan met jouw naam

Open `LICENSE` en vervang `tci-streamer contributors` door je eigen naam (of
laat staan als je het generiek wilt houden):

```diff
- Copyright (c) 2026 tci-streamer contributors
+ Copyright (c) 2026 Joeri PE5JW
```

---

## Stap 2 — Maak een GitHub-repository

1. Ga naar https://github.com/new
2. **Repository name**: `tci-streamer`
3. **Description**: "TCI proxy met bandbreedte-compressie voor remote ham radio (Thetis/Zeus)"
4. **Public** (anders kun je geen vrije CI gebruiken voor open source)
5. **NIET** "Initialize this repository with a README" — die hebben we al
6. **NIET** een .gitignore-template of LICENSE-template kiezen — die hebben we al
7. Klik **Create repository**

GitHub geeft je nu een URL zoals `git@github.com:USER/tci-streamer.git` (SSH)
of `https://github.com/USER/tci-streamer.git` (HTTPS). Onthoud die.

---

## Stap 3 — Initialiseer lokaal en push

Open een command-prompt in de tci-streamer-projectmap:

```cmd
cd C:\project\tci-streamer\tci-streamer

REM Init een nieuwe git-repository
git init

REM Voeg alle bestanden toe (excluded door .gitignore worden overgeslagen)
git add .

REM Eerste check: wat zou er gecommit worden?
git status
```

Controleer dat er **geen** ongewenste bestanden tussen staan:
- `target/` map → moet uitgesloten zijn ✓
- `vendor/opus/` met .lib/.dll → moet uitgesloten zijn ✓
- `.cargo/config.toml` → moet uitgesloten zijn ✓
- `*.wav` testopnames in `test/` → uitgesloten ✓

Als dat klopt:

```cmd
git commit -m "Initial public release: tci-streamer v0.1.23"

REM Voeg GitHub als remote toe (gebruik jouw URL)
git remote add origin git@github.com:USER/tci-streamer.git

REM Push (de eerste keer met -u om de branch te tracken)
git branch -M main
git push -u origin main
```

Bij de eerste push word je mogelijk gevraagd om SSH-host te accepteren.
Typ `yes`.

---

## Stap 4 — Verifieer op GitHub

Ga naar `https://github.com/USER/tci-streamer` en check:

- [ ] README rendert netjes (met badges en architectuur-diagram)
- [ ] `LICENSE` wordt herkend ("MIT License" verschijnt rechtsboven)
- [ ] Mappenstructuur klopt: `src/`, `docs/`, `.github/`, etc.
- [ ] `.github/workflows/ci.yml` staat erin
- [ ] **Géén** `target/`, `vendor/opus/`, of `.cargo/config.toml` zichtbaar

### Vervang `USER` in de README-badges

In `README.md` heb je 'USER' staan in de CI-badge URL. Vervang dat door je
echte GitHub-username:

```cmd
REM Bijvoorbeeld als je username 'pe5jw' is:
sed -i 's|USER/tci-streamer|pe5jw/tci-streamer|g' README.md
git add README.md
git commit -m "README: badges naar juiste GitHub URL"
git push
```

(Op Windows zonder sed: open README.md in een editor en vervang met de hand.)

---

## Stap 5 — Wacht op de eerste CI-run

GitHub Actions begint automatisch nadat de push klaar is.

1. Ga naar je repo → tab **Actions**
2. Je ziet een job "CI" lopen op je eerste commit
3. Wacht ~5-10 minuten — drie jobs draaien:
   - `cargo check (headless)` — snel, valt om als syntax kapot is
   - `Full build (Linux)` — bouwt incl. libopus en GUI's
   - `Full build (Windows)` — idem op Windows

**Als alles groen is:** klaar. **Als er rood verschijnt:**
- Klik op de gefaalde job → zie de log
- Meest waarschijnlijk: een ontbrekende system-dep (Linux) of een
  setup-script issue (Windows). Pas aan, commit, push, en herhaal.

---

## Stap 6 — Maak een eerste GitHub Release

Een GitHub Release maakt downloadbare binaries beschikbaar zonder dat
gebruikers het project zelf hoeven te bouwen.

```cmd
REM Tag de huidige commit met de versie (matcht Cargo.toml)
git tag -a v0.1.23 -m "tci-streamer 0.1.23"
git push origin v0.1.23
```

De `release.yml` workflow activeert automatisch op een `v*.*.*` tag:
1. Ga naar **Actions** tab → zie "Release" job lopen
2. Workflow bouwt Windows + Linux binaries
3. Maakt automatisch een GitHub Release met:
   - `tci-streamer-v0.1.23-windows-x64.zip` (alle 5 .exe's + README/LICENSE)
   - `tci-streamer-v0.1.23-linux-x64.tar.gz` (alle 5 binaries + README/LICENSE)
   - Release-notes uit CHANGELOG.md
4. Check op `https://github.com/USER/tci-streamer/releases`

---

## Stap 7 — Repository-instellingen aanvullen

Op de repo-pagina, klik **Settings** (rechtsboven):

### About (rechterkolom op de repo-pagina, tandwiel)
- Description: "TCI proxy met bandbreedte-compressie voor remote ham radio"
- Website: laat leeg, of link naar je QRZ-pagina
- Topics: voeg toe — `ham-radio`, `sdr`, `tci`, `thetis`, `zeus`, `rust`, `remote`

### Settings → General → Features
- ✓ Issues
- ✓ Discussions (optioneel, voor vragen die geen bug zijn)
- ✗ Wiki (overbodig, README + docs/ is genoeg)
- ✗ Projects (overbodig voor klein hobbyproject)

### Settings → General → Pull Requests
- ✓ Allow merge commits
- ✓ Automatically delete head branches (houdt de fork-lijst schoon)

### Settings → Branches
Voor een hobbyproject hoef je geen branch-protection toe te voegen. Voor een
serieuzer project: voeg toe op `main`:
- Require pull request before merging
- Require status checks to pass (kies de `check` job uit `ci.yml`)

---

## Stap 8 — Aankondigen (optioneel)

Plekken waar mensen voor tci-streamer interessant zouden zijn:

- **QRZ.com** forums (Remote Operating, Software)
- **Reddit**: r/amateurradio
- **groups.io** lijsten: Thetis, Expert Electronics SDR, remote-ham
- **Hackaday.io** project entry — geeft je een vaste link voor blog posts
- **PE-DX cluster / Dutch ham forums** (DKARS, VERON nieuwsbrieven)

In de aankondiging vermeld minstens:
- Wat het doet (één zin)
- Voor welke radio's (Thetis/Zeus, mogelijk anderen via TCI)
- Compressie-cijfers (15 Mbit/s → 110 kbit/s)
- Link naar de Release-page (niet repo, dan kunnen ze meteen downloaden)

---

## Onderhoud na publicatie

### Nieuwe release maken
1. Werk Cargo.toml bij naar de nieuwe versie (bv. 0.1.24)
2. Voeg een nieuwe sectie `## [0.1.24]` bovenaan CHANGELOG.md toe
3. Commit, push
4. Tag: `git tag -a v0.1.24 -m "..."` en `git push origin v0.1.24`
5. Release-workflow doet de rest automatisch

### Issues en PRs afhandelen
- Issues hebben een template (bug / feature) → makkelijk te triagen
- PRs hebben een template met checklist → makkelijk te reviewen
- CI moet groen blijven voor je een PR merge't

### Beveiligingsupdates
- GitHub stuurt automatisch Dependabot-alerts bij CVE in dependencies
- Voor Rust: vaak gewoon `cargo update` + check dat CI groen is + nieuwe release
- Voor libopus: volg Xiph.org releases (zelden security-issues)

---

## Snelle checklist (afvinken bij doen)

- [ ] Stap 0: Git + SSH-key geconfigureerd
- [ ] Stap 1: LICENSE naam aangepast
- [ ] Stap 2: GitHub-repo aangemaakt
- [ ] Stap 3: `git init` + push gedaan
- [ ] Stap 4: README badges username aangepast
- [ ] Stap 5: eerste CI-run groen
- [ ] Stap 6: eerste v0.1.23 release op GitHub zichtbaar
- [ ] Stap 7: About + topics ingevuld
- [ ] Stap 8: aangekondigd (optioneel)

Succes! 73, en happy DXing.
