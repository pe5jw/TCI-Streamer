# setup-libopus-windows.ps1
#
# tci-streamer v0.1.23 - libopus setup voor Windows
#
# Download libopus 1.5.2 source en bouw als statische library voor MSVC.
#
# Gebruik:
#   pwsh -ExecutionPolicy Bypass -File setup-libopus-windows.ps1
#
# Vereist: Visual Studio Build Tools (MSVC) + CMake (komt mee met VS).
# CMake hoeft niet in PATH te staan; we zoeken hem automatisch.

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$VendorDir = Join-Path $ScriptDir "vendor"
$BuildDir  = Join-Path $VendorDir "opus-build"
$InstallDir = Join-Path $VendorDir "opus"

$OpusVersion = "1.5.2"
$OpusUrl = "https://downloads.xiph.org/releases/opus/opus-$OpusVersion.tar.gz"
$OpusTarball = Join-Path $VendorDir "opus-$OpusVersion.tar.gz"
$OpusSrc = Join-Path $VendorDir "opus-$OpusVersion"

Write-Host "==> tci-streamer v0.1.23 - libopus setup voor Windows" -ForegroundColor Cyan

# ============================================================
# Functie: schrijf .cargo/config.toml
# Doet zowel env vars (voor audiopus_sys build script) als
# rustflags (voor de linker search path), zodat libopus zowel
# gevonden als gelinkt wordt.
# ============================================================
function Write-CargoConfig {
    $CargoConfigDir = Join-Path $ScriptDir ".cargo"
    if (-not (Test-Path $CargoConfigDir)) {
        New-Item -ItemType Directory -Path $CargoConfigDir | Out-Null
    }
    $CargoConfigPath = Join-Path $CargoConfigDir "config.toml"
    $LibPathForToml = (Join-Path $InstallDir "lib").Replace("\", "\\")
    $ConfigContent = @"
# Auto-gegenereerd door setup-libopus-windows.ps1
# Vertelt audiopus_sys onze gevendord libopus te gebruiken en zorgt
# dat de Rust linker hem ook vindt.

[env]
OPUS_LIB_DIR = "$LibPathForToml"
OPUS_NO_PKG  = "1"
OPUS_STATIC  = "1"

# Voeg de libopus lib-directory toe aan de Rust linker search path,
# zodat 'opus.lib' gevonden wordt bij static linking.
[target.x86_64-pc-windows-msvc]
rustflags = ["-L", "native=$LibPathForToml"]

[target.i686-pc-windows-msvc]
rustflags = ["-L", "native=$LibPathForToml"]

[target.aarch64-pc-windows-msvc]
rustflags = ["-L", "native=$LibPathForToml"]
"@
    Set-Content -Path $CargoConfigPath -Value $ConfigContent -Encoding UTF8
    Write-Host "  .cargo/config.toml geschreven" -ForegroundColor Green
}

# Sla over als al gebouwd, maar herschrijf wel altijd .cargo/config.toml
# zodat upgrades van het script een actuele config krijgen.
if (Test-Path (Join-Path $InstallDir "lib\opus.lib")) {
    Write-Host "libopus al aanwezig in $InstallDir - alleen .cargo/config.toml herschrijven." -ForegroundColor Green
    Write-CargoConfig
    exit 0
}

# ============================================================
# CMake locatie zoeken
# ============================================================
function Find-CMake {
    # 1) PATH check
    $cmd = Get-Command cmake -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Source
    }

    # 2) Probeer vswhere.exe (komt mee met VS Installer)
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        Write-Host "  vswhere gevonden, zoeken naar VS installaties..." -ForegroundColor DarkGray
        $vsInstalls = & $vswhere -all -prerelease -property installationPath 2>$null
        foreach ($vsPath in $vsInstalls) {
            $candidate = Join-Path $vsPath "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
            if (Test-Path $candidate) {
                return $candidate
            }
        }
    }

    # 3) Glob over bekende Visual Studio paden
    $vsRoots = @(
        "${env:ProgramFiles}\Microsoft Visual Studio",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio"
    ) | Where-Object { $_ -and (Test-Path $_) }

    foreach ($root in $vsRoots) {
        # Zoek alle versies (2017, 2019, 2022, 18, ...) en alle edities
        $candidates = Get-ChildItem -Path $root -Directory -ErrorAction SilentlyContinue | ForEach-Object {
            Get-ChildItem -Path $_.FullName -Directory -ErrorAction SilentlyContinue | ForEach-Object {
                Join-Path $_.FullName "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
            }
        }
        foreach ($c in $candidates) {
            if (Test-Path $c) {
                return $c
            }
        }
    }

    # 4) Standalone CMake installatie locaties
    $standaloneCandidates = @(
        "${env:ProgramFiles}\CMake\bin\cmake.exe",
        "${env:ProgramFiles(x86)}\CMake\bin\cmake.exe",
        "${env:ChocolateyInstall}\bin\cmake.exe"
    ) | Where-Object { $_ -and (Test-Path $_) }

    if ($standaloneCandidates) {
        return $standaloneCandidates[0]
    }

    return $null
}

Write-Host "==> CMake zoeken..." -ForegroundColor Cyan
$CMake = Find-CMake
if (-not $CMake) {
    Write-Host ""
    Write-Host "[FOUT] CMake niet gevonden." -ForegroundColor Red
    Write-Host ""
    Write-Host "CMake is nodig om libopus te bouwen. Twee opties:" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  Optie 1 - Via Visual Studio Installer (aanbevolen):"
    Write-Host "    1. Open 'Visual Studio Installer'"
    Write-Host "    2. Klik 'Modify' op je VS installatie"
    Write-Host "    3. Tab 'Individual components' -> zoek 'C++ CMake tools for Windows' -> aanvinken"
    Write-Host "    4. Klik 'Modify' rechtsonder en wacht tot installatie klaar is"
    Write-Host ""
    Write-Host "  Optie 2 - Standalone CMake:"
    Write-Host "    Download van https://cmake.org/download/ (kies 'Windows x64 Installer')"
    Write-Host "    Vink tijdens installatie 'Add CMake to system PATH' aan"
    Write-Host ""
    exit 1
}
Write-Host "  CMake gevonden: $CMake" -ForegroundColor Green

if (-not (Test-Path $VendorDir)) { New-Item -ItemType Directory -Path $VendorDir | Out-Null }

# ============================================================
# Download tarball
# ============================================================
if (-not (Test-Path $OpusTarball)) {
    Write-Host "==> Downloaden libopus $OpusVersion..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri $OpusUrl -OutFile $OpusTarball -UseBasicParsing
}

# ============================================================
# Uitpakken (tar zit standaard in Windows 10+)
# ============================================================
if (-not (Test-Path $OpusSrc)) {
    Write-Host "==> Uitpakken..." -ForegroundColor Cyan
    Push-Location $VendorDir
    try {
        & tar -xzf $OpusTarball
        if ($LASTEXITCODE -ne 0) { throw "tar uitpakken mislukt" }
    } finally {
        Pop-Location
    }
}

# ============================================================
# CMake configure + build + install
# ============================================================
Write-Host "==> CMake configureren..." -ForegroundColor Cyan
if (-not (Test-Path $BuildDir)) { New-Item -ItemType Directory -Path $BuildDir | Out-Null }

# CMAKE_POLICY_VERSION_MINIMUM=3.5 zorgt dat oude libopus CMakeLists.txt
# geaccepteerd wordt door moderne CMake (3.30+ / 4.0+).
#
# Belangrijk: we bouwen de argumenten als string-array zodat PowerShell
# 'm niet als nummer interpreteert (regionale locale-issue waarbij "3.5"
# door PowerShell wordt opgesplitst in "3" en ".5").
$CMakeArgs = @(
    '-S', "$OpusSrc",
    '-B', "$BuildDir",
    '-DCMAKE_POLICY_VERSION_MINIMUM=3.5',
    '-DCMAKE_BUILD_TYPE=Release',
    '-DBUILD_SHARED_LIBS=OFF',
    '-DOPUS_BUILD_SHARED_LIBRARY=OFF',
    '-DOPUS_BUILD_PROGRAMS=OFF',
    '-DOPUS_BUILD_TESTING=OFF',
    "-DCMAKE_INSTALL_PREFIX=$InstallDir"
)
& $CMake @CMakeArgs
if ($LASTEXITCODE -ne 0) { throw "CMake configureren mislukt" }

Write-Host "==> Bouwen libopus (kan 1-3 minuten duren)..." -ForegroundColor Cyan
& $CMake --build "$BuildDir" --config Release --parallel
if ($LASTEXITCODE -ne 0) { throw "CMake build mislukt" }

Write-Host "==> Installeren naar $InstallDir..." -ForegroundColor Cyan
& $CMake --install "$BuildDir" --config Release
if ($LASTEXITCODE -ne 0) { throw "CMake install mislukt" }

# Sommige libopus-versies installeren als 'opus.lib' in lib/, andere als 'libopus.lib'.
# Normaliseer naar 'opus.lib' want dat is wat de Rust crate verwacht.
$LibFile = Get-ChildItem -Path (Join-Path $InstallDir "lib") -Filter "*.lib" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($LibFile -and $LibFile.Name -ne "opus.lib") {
    Copy-Item $LibFile.FullName (Join-Path $InstallDir "lib\opus.lib")
    Write-Host "  $($LibFile.Name) gekopieerd naar opus.lib" -ForegroundColor Yellow
}

# ============================================================
# Schrijf .cargo/config.toml (zelfde logica als skip-pad bovenaan)
# ============================================================
Write-CargoConfig

Write-Host ""
Write-Host "==> libopus succesvol gebouwd!" -ForegroundColor Green
Write-Host "    Library: $InstallDir\lib\opus.lib"
Write-Host "    Headers: $InstallDir\include\opus\"
Write-Host ""
Write-Host "Nu kun je 'cargo build --release' draaien." -ForegroundColor Green
