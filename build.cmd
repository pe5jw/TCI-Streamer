@echo off
REM ============================================================
REM   tci-streamer v0.1.23 - Windows build script
REM
REM   Doet automatisch:
REM     1) libopus setup (alleen eerste keer, daarna geskipt)
REM     2) cargo build --release
REM     3) Toont de gegenereerde binaries
REM
REM   Gebruik: dubbelklik op build.cmd of run vanuit cmd/PowerShell:
REM     build.cmd            (release build)
REM     build.cmd debug      (debug build, sneller compileren)
REM     build.cmd clean      (alle build artifacts opruimen)
REM ============================================================

setlocal EnableDelayedExpansion

set SCRIPT_DIR=%~dp0
cd /d "%SCRIPT_DIR%"

set MODE=release
if /I "%~1"=="debug" set MODE=debug
if /I "%~1"=="clean" goto :clean

echo ==========================================================
echo   tci-streamer v0.1.23 build (%MODE%)
echo ==========================================================
echo.

REM --- Check of cargo beschikbaar is ---
where cargo >nul 2>&1
if errorlevel 1 (
    echo [FOUT] cargo niet gevonden in PATH.
    echo        Installeer Rust via https://rustup.rs/ en herstart cmd.
    exit /b 1
)

REM --- Stap 1: libopus + .cargo/config.toml opzetten ---
REM    De PS1 doet zelf de skip-check als libopus al bestaat,
REM    maar herschrijft altijd .cargo/config.toml zodat upgrades
REM    van het script direct de nieuwste linker-flags krijgen.
echo [1/2] libopus + cargo config check...
echo.
where pwsh >nul 2>&1
if errorlevel 1 (
    REM PowerShell 7 niet aanwezig, val terug op Windows PowerShell
    powershell -ExecutionPolicy Bypass -File "%SCRIPT_DIR%setup-libopus-windows.ps1"
) else (
    pwsh -ExecutionPolicy Bypass -File "%SCRIPT_DIR%setup-libopus-windows.ps1"
)
if errorlevel 1 (
    echo.
    echo [FOUT] libopus setup is mislukt. Zie de output hierboven.
    exit /b 1
)
echo.

REM --- Stap 2: cargo build ---
echo [2/2] Cargo build (%MODE%)...
echo.
if /I "%MODE%"=="release" (
    cargo build --release
) else (
    cargo build
)
if errorlevel 1 (
    echo.
    echo [FOUT] Cargo build is mislukt.
    exit /b 1
)

echo.
echo ==========================================================
echo   Build geslaagd!
echo ==========================================================
echo.
echo Binaries staan in: %SCRIPT_DIR%target\%MODE%\
echo.
for %%B in (tci-streamer-server.exe tci-streamer-client.exe fake-tci-server.exe tci-viewer.exe tci-launcher.exe) do (
    if exist "%SCRIPT_DIR%target\%MODE%\%%B" (
        for %%F in ("%SCRIPT_DIR%target\%MODE%\%%B") do (
            echo   %%B   ^(%%~zF bytes^)
        )
    ) else (
        echo   %%B   ^(ontbreekt^)
    )
)
echo.
echo Voorbeeld commando's:
echo   target\%MODE%\tci-streamer-server.exe --help
echo   target\%MODE%\tci-streamer-client.exe --help
echo   target\%MODE%\fake-tci-server.exe     --help     ^(emuleert Thetis^)
echo   target\%MODE%\tci-viewer.exe          --help     ^(grafische viewer^)
echo   target\%MODE%\tci-launcher.exe                   ^(grafische launcher^)
echo.
echo Tip: snellere build zonder de grafische viewer:
echo      cargo build --release --no-default-features
echo.
exit /b 0

:clean
echo Opruimen build artifacts...
if exist "%SCRIPT_DIR%target" (
    rmdir /s /q "%SCRIPT_DIR%target"
    echo   target\ verwijderd
)
if exist "%SCRIPT_DIR%vendor\opus-build" (
    rmdir /s /q "%SCRIPT_DIR%vendor\opus-build"
    echo   vendor\opus-build\ verwijderd
)
echo.
echo Bewaard:
echo   vendor\opus\        (libopus blijft staan, scheelt 1-3 min bij volgende build)
echo   .cargo\config.toml  (cargo configuratie blijft staan)
echo.
echo Voor volledige reset, verwijder ook vendor\opus\ handmatig.
exit /b 0
