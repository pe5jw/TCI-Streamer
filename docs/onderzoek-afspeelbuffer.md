# Onderzoek: waarom RadioBridge soepel is en tci-streamer rateligt

**Datum:** 21 mei 2026
**Aanleiding:** RadioBridge (C#/.NET, ongecomprimeerde PCM over TCP) klinkt
soepel zonder ratel. tci-streamer (Rust, Opus/FLAC over WebSocket) heeft een
hardnekkige ratel. Beide doen in essentie hetzelfde: audio van de ene PC naar
de andere brengen. Dit document analyseert wat RadioBridge anders doet in de
afspeel/buffer-keten en welke daarvan toepasbaar zijn op tci-streamer.

---

## 0. Samenvatting vooraf

Het belangrijkste architecturale verschil zit **niet** in de codec, maar in
**wie de audio afspeelt en hoe die wordt gebufferd**:

- **RadioBridge speelt de audio zelf af.** Het heeft een echte
  `BufferedWaveProvider` (NAudio jitter-buffer) tussen het netwerk en de
  geluidskaart. Die buffer absorbeert timing-variaties: pakketten komen
  ongelijkmatig binnen, maar de geluidskaart trekt er gelijkmatig uit. Dat is
  precies wat een jitter-buffer hoort te doen.

- **tci-streamer speelt niets af.** De client decodeert de audio en stuurt elke
  frame **meteen door naar THRA** over een tweede WebSocket. THRA is dan
  verantwoordelijk voor de afspeelbuffer. tci-streamer heeft geen enkele
  jitter-buffer; het is een doorgeefluik dat afhankelijk is van hoe goed THRA
  ongelijkmatig getimede frames opvangt.

**De ratel komt dus waarschijnlijk doordat THRA frames op onregelmatige
tijdstippen aangeleverd krijgt** en zelf geen ruime jitter-buffer heeft (of een
die we onbedoeld verstoren). RadioBridge omzeilt dit door zelf een nette
buffer aan te houden voor de geluidskaart.

De meest kansrijke fix voor tci-streamer is dan ook: **een jitter-buffer met
gelijkmatige uitlevering toevoegen op de client, vóórdat de audio naar THRA
gaat** — het equivalent van RadioBridge's `BufferedWaveProvider`.

---

## 1. De twee architecturen naast elkaar

### RadioBridge (werkt soepel)

```
[mic/VAC] → WaveInEvent → [noise filter] → TCP send
                                              │
                                              ▼  (netwerk, jitter)
                                              │
TCP recv → BufferedWaveProvider → WaveOutEvent → [speaker/VAC]
            ^^^^^^^^^^^^^^^^^^^^
            jitter-buffer met
            ruime BufferDuration
            (PlaybackLatencyMs × 4)
```

Cruciaal: de geluidskaart (`WaveOutEvent`) trekt **op zijn eigen, vaste klok**
samples uit de `BufferedWaveProvider`. Als er even geen netwerkpakket is, speelt
de buffer door wat er nog in zit. Als er een burst pakketten komt, worden die
opgevangen (`DiscardOnBufferOverflow = true`). De geluidskaart hoort altijd een
gelijkmatige stroom.

### tci-streamer (rateligt)

```
[Thetis] → server: TCI → Opus/FLAC encode → WebSocket
                                               │
                                               ▼  (netwerk, jitter)
                                               │
client: WebSocket recv → decode → bouw TCI frame → WebSocket naar THRA
                                                     │
                                                     ▼
                                                   [THRA speelt af]
```

tci-streamer heeft **geen buffer met een eigen klok**. Elke gedecodeerde frame
wordt direct doorgestuurd zodra hij binnenkomt. De timing waarmee THRA frames
ontvangt is exact de timing waarmee ze over het netwerk aankwamen — inclusief
alle jitter. THRA moet dat zelf opvangen.

---

## 2. Wat RadioBridge concreet goed doet

### 2.1 Een echte jitter-buffer (`BufferedWaveProvider`)

```csharp
_playbackBuffer = new BufferedWaveProvider(fmt)
{
    BufferDuration          = TimeSpan.FromMilliseconds(_preset.PlaybackLatencyMs * 4),
    DiscardOnBufferOverflow = true,
};
```

- `BufferDuration = PlaybackLatencyMs × 4` → ruime marge. Bij 50ms latency-preset
  is dat 200ms buffer. Dat is genoeg om normale netwerk-jitter glad te strijken.
- `DiscardOnBufferOverflow = true` → bij een burst worden oude samples gedropt
  in plaats van de buffer eindeloos te laten groeien (wat tot oplopende latency
  zou leiden).

### 2.2 De geluidskaart bepaalt het tempo, niet het netwerk

`WaveOutEvent` met `DesiredLatency = PlaybackLatencyMs` trekt op een vaste
hardware-klok uit de buffer. Dit ontkoppelt de **ontvangst-timing** (netwerk,
ongelijkmatig) van de **afspeel-timing** (geluidskaart, perfect gelijkmatig).
Dit is de kern van waarom het soepel klinkt.

### 2.3 Frames worden niet herverpakt

```csharp
private void OnDataAvailable(object? sender, WaveInEventArgs e)
{
    ...
    _stream.Write(BitConverter.GetBytes(e.BytesRecorded), 0, 4);
    _stream.Write(e.Buffer, 0, e.BytesRecorded);
}
```

De PCM-buffer gaat 1:1 het netwerk op zoals de geluidskaart hem levert. Geen
opknippen in vaste chunks, geen herverpakken. (Dit was precies de fout in
tci-streamer v0.1.12: het herknippen op 2048-sample grenzen creëerde
discontinuïteiten op 23,4 Hz.)

### 2.4 TCP met `NoDelay` (Nagle uit)

```csharp
_tcpClient.NoDelay = true;
```

Voorkomt dat kleine pakketten worden opgespaard (Nagle's algoritme). Lagere,
constantere latency per pakket — minder jitter.

---

## 3. Wat tci-streamer mist

| Aspect | RadioBridge | tci-streamer |
|---|---|---|
| Jitter-buffer | Ja (`BufferedWaveProvider`, ~200ms) | Nee |
| Eigen afspeelklok | Ja (geluidskaart) | Nee (THRA doet afspelen) |
| Overflow-beleid | `DiscardOnBufferOverflow` | n.v.t. |
| Frame-herverpakking | Nee (1:1 doorgeven) | v0.1.13+ niet meer; ok |
| Nagle uit | Ja (`NoDelay`) | WebSocket/TCP — niet expliciet gezet |

Het grote gat is de **ontbrekende jitter-buffer met eigen klok**. tci-streamer
levert frames aan THRA op netwerk-timing. Als de jitter groter is dan THRA's
interne buffer aankan, hoor je dat als ratel/onderbrekingen.

---

## 4. Waarom dit lastiger is voor tci-streamer dan voor RadioBridge

RadioBridge **is** het eindpunt — het praat direct met de geluidskaart, dus het
kan een geluidskaart-klok als tempogever gebruiken. Dat is de natuurlijke plek
voor een jitter-buffer.

tci-streamer is een **proxy** — het eindpunt is THRA, een aparte applicatie.
tci-streamer praat met THRA over TCI (WebSocket), niet met een geluidskaart.
Er is dus geen hardware-klok in tci-streamer om de uitlevering op te timen.

Dat betekent dat een jitter-buffer in tci-streamer **een eigen software-klok**
nodig heeft (een timer) om frames gelijkmatig naar THRA te sturen. Dat is goed
te doen, maar het is een wezenlijk andere aanpak dan "stuur door zodra
ontvangen".

---

## 5. Voorgestelde oplossingsrichtingen voor tci-streamer

### Optie A — Software jitter-buffer met timer-gestuurde uitlevering (aanbevolen)

Bouw op de client een buffer tussen "Opus/FLAC gedecodeerd" en "verstuur naar
THRA":

1. Gedecodeerde audio gaat in een ringbuffer (bv. 100-200ms diep).
2. Een tokio-timer tikt op een vast interval (bv. elke 20ms) en stuurt dan
   precies één frame van vaste grootte naar THRA.
3. Bij onderloop (buffer leeg): stuur stilte of de laatste frame nogmaals
   (concealment). Bij overloop: drop oudste samples.

**Voordeel:** dit is exact wat RadioBridge's `BufferedWaveProvider` doet, maar
dan met een software-klok. THRA krijgt een perfect gelijkmatige stroom.

**Nadeel:** voegt latency toe (de buffer-diepte) en is meer code. Het knippen op
vaste frame-grootte mag NIET de v0.1.12-fout herintroduceren — daarom moet de
frame-grootte matchen met wat THRA verwacht (Thetis' `audio_stream_samples`,
typisch 2048), en moet de timer-tik exact overeenkomen met de afspeelduur van
die frame (2048 @ 48kHz = 42,67ms).

### Optie B — Onderzoek of THRA's eigen buffer instelbaar is

Als THRA een instelbare audio-buffer/jitter-instelling heeft, is het wellicht
voldoende die ruimer te zetten. Dan hoeft tci-streamer niets te doen. Dit is de
goedkoopste fix als die knop bestaat.

### Optie C — Stuur grotere frames minder vaak

In plaats van elke kleine Opus-frame (10-40ms) direct door te sturen, accumuleer
tot een grotere natuurlijke eenheid en stuur die. LET OP: dit lijkt op de
v0.1.12 rebuffer die juist ratel veroorzaakte. Het verschil moet zijn dat de
uitlevering **gelijkmatig getimed** is (optie A), niet zomaar op grootte
geknipt. Grootte-knippen zonder timing = ratel; timing-gestuurde uitlevering =
soepel.

---

## 6. Belangrijk inzicht: grootte vs. timing

De v0.1.12 ratel leerde ons: **frames opknippen op een vaste grootte zonder
aandacht voor timing veroorzaakt discontinuïteiten.** RadioBridge laat zien wat
het wél goed doet: het knipt niet op grootte, maar laat een klok (de
geluidskaart) op vast tempo uit een buffer trekken.

De les voor tci-streamer: het gaat niet om de chunk-grootte op zich, maar om
**gelijkmatige uitlevering op een vaste klok**. Een jitter-buffer met
timer-gestuurde uitlevering (optie A) is het software-equivalent van wat
RadioBridge gratis krijgt van de geluidskaart-hardware.

---

## 7. Andere RadioBridge-details die de moeite waard zijn

- **Noise suppression (RNNoise + noise-gate fallback).** RadioBridge heeft een
  neurale ruisfilter (RNNoise, 48kHz mono, 480-sample frames) met een
  noise-gate als fallback. tci-streamer heeft geen ruisonderdrukking. Voor
  SSB-spraak kan dit waarde toevoegen, los van de ratel-kwestie.

- **Config-handshake.** RadioBridge stuurt een JSON `BridgeConfig` bij verbinden
  (sample rate, bits, channels, buffer-ms, noise on/off). tci-streamer heeft een
  vergelijkbaar Hello-mechanisme. Geen actie nodig; ter info.

- **`DiscardOnBufferOverflow`-mentaliteit.** RadioBridge kiest expliciet voor
  "liever audio droppen dan latency laten oplopen". Dat is de juiste keuze voor
  real-time radio. Een jitter-buffer in tci-streamer zou hetzelfde beleid moeten
  voeren.

---

## 8. Conclusie

RadioBridge is niet soepel doordat het ongecomprimeerd is — het is soepel doordat
het een **echte jitter-buffer met een eigen afspeelklok** heeft (NAudio's
`BufferedWaveProvider` + `WaveOutEvent`). tci-streamer mist dat volledig: het is
een doorgeefluik dat netwerk-jitter ongefilterd aan THRA doorgeeft.

De meest kansrijke volgende stap is **optie A**: een software jitter-buffer op
de client met timer-gestuurde, gelijkmatige uitlevering naar THRA, met
frame-grootte gelijk aan Thetis' `audio_stream_samples` (2048) en een tik-tempo
gelijk aan de afspeelduur daarvan (42,67ms). Dit combineert het beste van beide:
de bandbreedte-voordelen van Opus/FLAC én de soepelheid van een echte
jitter-buffer.

Voordat we bouwen is het de moeite waard om eerst **optie B** te checken: als
THRA een instelbare audio-buffer heeft, is dat de goedkoopste oplossing.
