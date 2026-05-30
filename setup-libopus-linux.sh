#!/bin/bash
# setup-libopus-linux.sh
#
# tci-streamer v0.1.23 - libopus setup voor Linux
#
# Op Linux is libopus zo standaard dat we gewoon de systeempakket gebruiken.
#
# Gebruik:
#   chmod +x setup-libopus-linux.sh
#   ./setup-libopus-linux.sh

set -e

echo "==> tci-streamer v0.1.23 - libopus setup voor Linux"

if pkg-config --exists opus; then
    OPUS_VER=$(pkg-config --modversion opus)
    echo "libopus al geinstalleerd (versie $OPUS_VER) - niets te doen."
    exit 0
fi

if command -v apt-get &> /dev/null; then
    echo "==> Installeren libopus-dev + eframe systeem-deps via apt..."
    sudo apt-get update
    # libopus-dev: voor de opus crate
    # pkg-config: voor cargo om libopus te vinden
    # libgl1-mesa-dev, libxkbcommon-dev: voor eframe (tci-viewer GUI)
    # libssl-dev: in case TLS feature gebruikt wordt
    sudo apt-get install -y libopus-dev pkg-config \
        libgl1-mesa-dev libxkbcommon-dev
elif command -v dnf &> /dev/null; then
    echo "==> Installeren opus-devel via dnf..."
    sudo dnf install -y opus-devel pkgconfig mesa-libGL-devel libxkbcommon-devel
elif command -v pacman &> /dev/null; then
    echo "==> Installeren opus via pacman..."
    sudo pacman -S --noconfirm opus pkgconf
elif command -v zypper &> /dev/null; then
    echo "==> Installeren libopus-devel via zypper..."
    sudo zypper install -y libopus-devel pkg-config
else
    echo "ERROR: Onbekende package manager. Installeer libopus development package handmatig."
    exit 1
fi

echo ""
echo "==> libopus succesvol geinstalleerd!"
echo "Nu kun je 'cargo build --release' draaien."
