# Bijdragen aan tci-streamer

Bedankt voor je interesse! Dit project is ontstaan uit de behoefte aan een
betrouwbare TCI-proxy voor remote ham radio gebruik, en bijdragen zijn welkom —
van kleine bug fixes tot grote nieuwe features.

## Vóór je begint

**Open eerst een issue.** Voor niet-triviale wijzigingen (nieuwe features,
ingrijpende refactors) bespreken we de aanpak het liefst eerst in een issue,
zodat je geen werk doet dat we later weer terug moeten draaien. Voor kleine
fixes of doc-updates kun je gewoon een PR openen.

## Ontwikkelomgeving opzetten

Je hebt nodig:
- **Rust** stable (1.75 of nieuwer, edition 2021)
- **libopus** — installeer via `setup-libopus-linux.sh` (Linux/Pi) of
  `setup-libopus-windows.ps1` (Windows). Zie README voor details.
- Optioneel: **Visual Studio Build Tools** (Windows, voor MSVC linker)

Klonen + bouwen:
```bash
git clone https://github.com/<jouw-fork>/tci-streamer.git
cd tci-streamer
./setup-libopus-linux.sh    # of de Windows variant
cargo build --release
```

Voor headless builds zonder GUI's:
```bash
cargo build --release --no-default-features
```

## Code style

- **`cargo fmt`** vóór elke commit (standaard Rust-formatter)
- **`cargo clippy`** zonder waarschuwingen waar mogelijk
- **Commentaar in het Nederlands of Engels** — het project heeft mixed taal
  door zijn oorsprong; consistent binnen één bestand blijven is genoeg.
- **Geen `unwrap()` in productiecode** — gebruik `?`, `anyhow::Context`, of
  expliciete error-paden. `unwrap()` is OK in tests en in `main()` voor
  one-shot fouten.

## Tests

```bash
cargo test --all-features
```

Bij wijzigingen aan de codec-pijplijn (`src/codec/`), de TCI-proto-laag
(`src/proto/`), of de launcher-parser (`src/launcher/`) zijn unit tests vrijwel
verplicht. De launcher-parser heeft bijvoorbeeld roundtrip-tests: een script
dat de launcher genereert moet de launcher zelf weer kunnen inlezen.

## End-to-end testen zonder radio

De test-keten in `test/test-keten.cmd` start de hele pipeline op één PC,
zonder Thetis/Zeus en zonder THRA:
```
fake-tci-server → tci-streamer-server → tci-streamer-client → tci-viewer
```
Gebruik dit voor het valideren van wijzigingen die meerdere binaries raken.

## Pull request checklist

- [ ] Branch vanaf `main`
- [ ] Eén logische wijziging per PR
- [ ] Commits hebben begrijpelijke messages (geen "fix" of "wip")
- [ ] `cargo fmt && cargo clippy` schoon
- [ ] `cargo test --all-features` slaagt
- [ ] Voor user-zichtbare wijzigingen: CHANGELOG bijgewerkt onder
      "Unreleased" (de maintainer release't en hernoemt)
- [ ] Voor nieuwe CLI-opties: README en `--help` text bijgewerkt

## Stijl van overleg

Discussies gaan over code, niet over personen. Reviewers wijzen op zaken die
beter kunnen; auteurs leggen uit waarom keuzes gemaakt zijn. Beide partijen
mogen zich vergissen — dat is normaal.

## Licentie van bijdragen

Bijdragen worden onder dezelfde licentie geaccepteerd als het project: MIT.
Door een PR in te dienen verklaar je dat je het recht hebt om de code onder
deze licentie aan te bieden.
