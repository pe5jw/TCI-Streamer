@echo off
REM ============================================================
REM  test-keten.cmd
REM
REM  Start de volledige tci-streamer test-keten op EEN PC,
REM  zonder Thetis of THRA:
REM
REM    fake-tci-server  (emuleert Thetis op :40001)
REM         |
REM    tci-streamer-server  (comprimeert, luistert :9001)
REM         |
REM    tci-streamer-client  (decomprimeert, luistert :40002)
REM         |
REM    tci-viewer  (toont audio + spectrum)
REM
REM  Elk onderdeel opent in een eigen venster. Sluit de vensters
REM  om te stoppen. Zet dit script in de map met de .exe's.
REM ============================================================

cd /d "%~dp0"

echo Start fake-tci-server (emuleert Thetis)...
start "fake-tci-server" fake-tci-server.exe --listen 127.0.0.1:40001
timeout /t 1 >nul

echo Start tci-streamer-server...
start "tci-streamer-server" tci-streamer-server.exe ^
    --upstream ws://127.0.0.1:40001 --listen 0.0.0.0:9001
timeout /t 1 >nul

echo Start tci-streamer-client...
start "tci-streamer-client" tci-streamer-client.exe ^
    --server ws://127.0.0.1:9001 --listen 127.0.0.1:40002 --codec opus --channels mono
timeout /t 2 >nul

echo Start tci-viewer...
start "tci-viewer" tci-viewer.exe --connect ws://127.0.0.1:40002

echo.
echo Keten gestart. In het tci-viewer venster zie je de audio-VU
echo en het spectrum. Sluit de vensters om te stoppen.
echo.
echo Tip: test IQ-swap door tci-streamer-server te herstarten met
echo      --iq-swap swap   (of conj). Het spectrum in tci-viewer
echo      moet dan spiegelen.
pause
