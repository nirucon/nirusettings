#!/usr/bin/env bash
# NIRUSETTINGS installer: builds this source tree with cargo and installs the
# program, its desktop entry, icon and schema files for the current user in
# ~/.local. It needs no root; sudo is only asked for to install missing
# build packages.
#
#   ./install.sh               install missing build packages (asks first),
#                              build and install
#   ./install.sh --yes         install missing packages without asking
#   ./install.sh --no-deps     never install system packages
#
# Build packages per distribution (installed with sudo when missing):
#   Debian/Ubuntu: cargo gcc
#   Arch:          rust gcc
#   Void:          cargo gcc
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
BIN="$HOME/.local/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DATA/applications"
ICONS="$DATA/icons/hicolor/scalable/apps"
SCHEMAS="$DATA/nirusettings/schema"
B=$'\033[1m'; G=$'\033[32m'; Y=$'\033[33m'; R=$'\033[31m'; N=$'\033[0m'

ASSUME_YES=0
DEPS=1
[[ ${NIRUSETTINGS_ASSUME_YES:-${NIRUX_ASSUME_YES:-0}} == 1 ]] && ASSUME_YES=1
for arg in "$@"; do
  case "$arg" in
    -y|--yes) ASSUME_YES=1 ;;
    --no-deps) DEPS=0 ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf "%sERROR%s unknown option: %s (try --help)\n" "$R" "$N" "$arg"; exit 2 ;;
  esac
done
[[ $(id -u) != 0 ]] || { echo "Run ./install.sh as your own user, not root." >&2; exit 1; }

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
printf "%sNIRUSETTINGS %s%s: the NIRUX settings, built from source\n\n" "$B" "$VERSION" "$N"

# A rustup toolchain in ~/.cargo/bin counts too.
[[ -x "$HOME/.cargo/bin/cargo" ]] && export PATH="$PATH:$HOME/.cargo/bin"

# Read only what is needed: sourcing os-release would overwrite VERSION.
OS_ID=unknown; OS_LIKE=""
if [[ -r /etc/os-release ]]; then
  OS_ID="$(sed -n 's/^ID=//p' /etc/os-release | tr -d '"')"
  OS_LIKE=" $(sed -n 's/^ID_LIKE=//p' /etc/os-release | tr -d '"') "
fi
if [[ "$OS_ID" =~ ^(arch|omarchy|cachyos|endeavouros|manjaro)$ ]] || [[ "$OS_LIKE" == *" arch "* ]]; then PM=pacman
elif [[ "$OS_ID" =~ ^(void|voidlinux)$ ]]; then PM=xbps
elif [[ "$OS_ID" =~ ^(debian|ubuntu|linuxmint|pop)$ ]] || [[ "$OS_LIKE" == *" debian "* ]] || [[ "$OS_LIKE" == *" ubuntu "* ]]; then PM=apt
else PM=unknown
fi
if [[ $PM == unknown ]]; then
  if command -v xbps-install >/dev/null 2>&1; then PM=xbps
  elif command -v pacman >/dev/null 2>&1; then PM=pacman
  elif command -v apt-get >/dev/null 2>&1; then PM=apt
  fi
fi

pkg() { # NAME -> the package for this package manager
  case "$PM:$1" in
    apt:cc|pacman:cc|xbps:cc) echo gcc ;;
    apt:cargo|xbps:cargo) echo cargo ;; pacman:cargo) echo rust ;;
  esac
}

missing=()
command -v cargo >/dev/null 2>&1 || missing+=(cargo)
# Rust links through the system C compiler.
command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || missing+=(cc)

if [[ ${#missing[@]} -gt 0 ]]; then
  packages=()
  for m in "${missing[@]}"; do p="$(pkg "$m")"; [[ -n $p ]] && packages+=("$p"); done
  printf "DEPENDENCIES (to build)\n"
  for m in "${missing[@]}"; do printf "  %s•%s %s\n" "$Y" "$N" "$(pkg "$m")"; done
  if [[ $DEPS == 0 || $PM == unknown || ${#packages[@]} -eq 0 ]]; then
    printf "\n%sMissing:%s %s\nInstall them with your package manager, then run ./install.sh again.\n" "$R" "$N" "${missing[*]}"
    printf "Debian/Ubuntu: cargo gcc   Arch: rust gcc   Void: cargo gcc\n"
    exit 1
  fi
  if [[ $ASSUME_YES == 0 ]]; then
    if [[ ! -t 0 ]]; then
      printf "%sPackages are missing and there is no terminal to ask for permission.%s\nRun in a terminal, or with --yes.\n" "$R" "$N"
      exit 1
    fi
    printf "\nInstall them with sudo? [Y/n] "
    read -r ans
    [[ ${ans:-Y} =~ ^[Yy]$ ]] || { printf "Installation cancelled.\n"; exit 1; }
  fi
  case $PM in
    pacman) sudo pacman -S --needed --noconfirm "${packages[@]}" ;;
    xbps) sudo xbps-install -Sy "${packages[@]}" ;;
    apt) sudo apt-get update; sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y "${packages[@]}" ;;
  esac
  hash -r
fi

# Rust 1.85 or newer (Debian 13 ships 1.85).
rust_ver="$( (rustc --version 2>/dev/null || true) | awk '{print $2}')"
if [[ -z $rust_ver ]] || [[ "$(printf '%s\n%s\n' 1.85.0 "$rust_ver" | sort -V | head -1)" != 1.85.0 ]]; then
  printf "%sERROR%s Rust 1.85 or newer is needed (found %s). Update it (rustup update) or the distribution's cargo.\n" "$R" "$N" "${rust_ver:-none}"
  exit 1
fi

printf "Building NIRUSETTINGS %s with Rust %s…\n" "$VERSION" "$rust_ver"
cargo build --release --locked
BINARY="${CARGO_TARGET_DIR:-$ROOT/target}/release/nirusettings"
"$BINARY" --version | grep -qx "NIRUSETTINGS $VERSION" || { printf "%sERROR%s the built program does not run.\n" "$R" "$N"; exit 1; }

mkdir -p "$BIN" "$APPS" "$ICONS" "$SCHEMAS"
# An atomic rename: a running NIRUSETTINGS keeps its old file.
install -m 0755 "$BINARY" "$BIN/.nirusettings.new.$$"
mv -f "$BIN/.nirusettings.new.$$" "$BIN/nirusettings"
install -m 0644 assets/nirusettings.svg "$ICONS/nirusettings.svg"
# The schema files of this version: NIRUSETTINGS reads them before its
# built-in copies, and NIRULAUNCHER can read the same files.
for f in schema/*.toml; do install -m 0644 "$f" "$SCHEMAS/$(basename "$f")"; done
sed -e "s|^Exec=nirusettings|Exec=$BIN/nirusettings|" assets/nirusettings.desktop > "$APPS/nirusettings.desktop"
chmod 0644 "$APPS/nirusettings.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$DATA/icons/hicolor" >/dev/null 2>&1 || true
printf "%s✓%s Installed %s\n" "$G" "$N" "$BIN/nirusettings"
printf "%s✓%s Desktop entry %s\n" "$G" "$N" "$APPS/nirusettings.desktop"

printf "%s✓%s Schema files %s\n" "$G" "$N" "$SCHEMAS"

command -v fc-match >/dev/null 2>&1 || \
  printf "%sNote:%s no fc-match (fontconfig): the DejaVu Sans Mono fallback is used.\n" "$Y" "$N"
command -v niructl >/dev/null 2>&1 || \
  printf "%sNote:%s no niructl: changes are written but applied at the next start.\n" "$Y" "$N"
printf "\nRun: nirusettings   (or nirusettings bar, lock, keys...; nirusettings --help)\n"
