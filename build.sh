#!/bin/bash
# ============================================================
#   tci-streamer v0.1.23 - Linux build script
#
#   Doet automatisch:
#     1) libopus systeempakket installeren (sudo apt install ...)
#     2) cargo build --release
#     3) Toont de gegenereerde binaries
#
#   Gebruik:
#     ./build.sh             (release build)
#     ./build.sh debug       (debug build, sneller compileren)
#     ./build.sh clean       (alle build artifacts opruimen)
# ============================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

MODE="release"
case "${1:-}" in
    debug) MODE="debug" ;;
    clean) MODE="clean" ;;
    "") MODE="release" ;;
    *)
        echo "Onbekende optie: $1"
        echo "Gebruik: $0 [debug|release|clean]"
        exit 1
        ;;
esac

if [ "$MODE" = "clean" ]; then
    echo "Opruimen build artifacts..."
    rm -rf "$SCRIPT_DIR/target"
    echo "  target/ verwijderd"
    echo
    echo "(libopus is een systeempakket, niet aangeraakt)"
    exit 0
fi

echo "=========================================================="
echo "  tci-streamer v0.1.23 build ($MODE)"
echo "=========================================================="
echo

# --- Check cargo ---
if ! command -v cargo &> /dev/null; then
    echo "[FOUT] cargo niet gevonden in PATH."
    echo "       Installeer Rust via https://rustup.rs/"
    exit 1
fi

# --- Stap 1: libopus check ---
if ! pkg-config --exists opus 2>/dev/null; then
    echo "[1/2] libopus niet gevonden, setup uitvoeren..."
    echo
    bash "$SCRIPT_DIR/setup-libopus-linux.sh"
    echo
else
    OPUS_VER=$(pkg-config --modversion opus)
    echo "[1/2] libopus systeempakket aanwezig (versie $OPUS_VER) - overslaan."
fi
echo

# --- Stap 2: cargo build ---
echo "[2/2] Cargo build ($MODE)..."
echo
if [ "$MODE" = "release" ]; then
    cargo build --release
else
    cargo build
fi

echo
echo "=========================================================="
echo "  Build geslaagd!"
echo "=========================================================="
echo
echo "Binaries staan in: $SCRIPT_DIR/target/$MODE/"
echo
for binary in tci-streamer-server tci-streamer-client fake-tci-server tci-viewer tci-launcher; do
    if [ -f "$SCRIPT_DIR/target/$MODE/$binary" ]; then
        SIZE=$(du -h "$SCRIPT_DIR/target/$MODE/$binary" | cut -f1)
        echo "  $binary   ($SIZE)"
    else
        echo "  $binary   (ontbreekt)"
    fi
done
echo
echo "Voorbeeld commando's:"
echo "  ./target/$MODE/tci-streamer-server --help"
echo "  ./target/$MODE/tci-streamer-client --help"
echo "  ./target/$MODE/fake-tci-server     --help     (emuleert Thetis)"
echo "  ./target/$MODE/tci-viewer          --help     (grafische viewer)"
echo "  ./target/$MODE/tci-launcher                   (grafische launcher)"
echo
echo "Tip: snellere build zonder grafische viewer (handig voor headless Pi):"
echo "     cargo build --release --no-default-features"
echo
