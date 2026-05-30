@echo off
REM ============================================================
REM  build-linux-via-wsl.cmd
REM
REM  Bouwt een Linux-binary van tci-streamer VANAF Windows,
REM  via WSL (Windows Subsystem for Linux).
REM
REM  Dit is veruit de betrouwbaarste manier om op een Windows-PC
REM  een Linux-binary te maken: WSL draait een echte Linux-
REM  omgeving, dus libopus en de Rust-toolchain werken net als
REM  op een echte Linux-machine. Geen cross-compile-ellende.
REM
REM  VEREISTEN (eenmalig):
REM    1. WSL geinstalleerd:   wsl --install
REM       (herstart daarna de PC)
REM    2. Een Linux-distro (Ubuntu is prima, standaard via wsl --install)
REM
REM  Dit script regelt de rest (Rust + libopus-dev binnen WSL).
REM ============================================================

setlocal
cd /d "%~dp0"

echo === tci-streamer: Linux-build via WSL ===
echo.

REM Check of WSL beschikbaar is
wsl --status >nul 2>&1
if errorlevel 1 (
    echo FOUT: WSL lijkt niet geinstalleerd.
    echo Installeer het eenmalig met:  wsl --install
    echo en herstart daarna de PC.
    pause
    exit /b 1
)

echo WSL gevonden. Build starten binnen Linux...
echo (eerste keer kan even duren: toolchain + libopus worden geinstalleerd)
echo.

REM Roep het bash-script aan binnen WSL. %cd% wordt automatisch
REM gemapt naar /mnt/c/... door WSL.
wsl bash ./build-linux.sh

if errorlevel 1 (
    echo.
    echo BUILD MISLUKT - zie meldingen hierboven.
    pause
    exit /b 1
)

echo.
echo === KLAAR ===
echo De Linux-binaries staan in:
echo   target\x86_64-unknown-linux-gnu\release\tci-streamer-server
echo   target\x86_64-unknown-linux-gnu\release\tci-streamer-client
echo.
echo Kopieer die naar je Linux-machine (chmod +x indien nodig).
pause
endlocal
