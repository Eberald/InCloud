#!/bin/sh
# Install InCloud for the current user: build it, put it in ~/.local/bin and add it to the
# application menu. Nothing is written outside your home folder and no root rights are needed.
#
#   ./install.sh              build and install
#   ./install.sh --update     take the new commits (git pull), build and install again
#   ./install.sh --uninstall  remove what this script installed (settings and runs are kept)

set -e
cd "$(dirname "$0")"

BIN="$HOME/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"

if [ "$1" = "--update" ]; then
    # take the new commits, then build and install as usual
    if [ -d .git ] && command -v git >/dev/null 2>&1; then
        git pull --ff-only
    fi
    shift
fi

if [ "$1" = "--uninstall" ]; then
    rm -f "$BIN/incloud" "$APPS/incloud.desktop" "$ICONS/incloud.png"
    echo "InCloud removed. Your settings (~/.config/incloud) and any CLOUDY it installed (~/.local/share/incloud) are kept."
    exit 0
fi

# Rust is needed only to build InCloud
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust is needed to build InCloud and is not installed."
    printf "Install it now from https://rustup.rs (only for your user)? [y/N] "
    read -r answer
    case "$answer" in
        y|Y|yes|Yes)
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
            PATH="$HOME/.cargo/bin:$PATH"
            ;;
        *)
            echo "Install Rust from https://rustup.rs and run this script again."
            exit 1
            ;;
    esac
fi
if ! command -v cc >/dev/null 2>&1 && [ ! -x tools/zig-cc ]; then
    echo "A C compiler (cc) is needed to link InCloud: install your system's build tools first,"
    echo "for example:  sudo apt install build-essential   or   sudo dnf install gcc"
    exit 1
fi

echo "Building InCloud (the first time takes a few minutes)…"
cargo build --release

mkdir -p "$BIN" "$APPS" "$ICONS"
install -m 755 target/release/incloud "$BIN/incloud"
"$BIN/incloud" --write-icon "$ICONS/incloud.png" >/dev/null

cat > "$APPS/incloud.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=InCloud
Comment=Set up, run and plot CLOUDY models
Exec=$BIN/incloud %F
Icon=$ICONS/incloud.png
Terminal=false
Categories=Science;Astronomy;Education;
StartupWMClass=incloud
EOF
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true

echo
echo "InCloud is installed: open it from the application menu, or type  incloud"
case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "(Add $BIN to your PATH to start it from a terminal.)" ;;
esac
