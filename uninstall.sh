#!/usr/bin/env bash
# Remove NIRUSETTINGS: the program, its desktop entry, icon and schema files.
# The settings it changed belong to the other programs and stay as they are.
#
#   ./uninstall.sh           keep its cache (fonts found) and state
#   ./uninstall.sh --purge   also remove ~/.cache/nirusettings and
#                            ~/.local/state/nirusettings
set -euo pipefail
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}"
APPS="$DATA/applications"

rm -f "$HOME/.local/bin/nirusettings" "$APPS/nirusettings.desktop" "$DATA/icons/hicolor/scalable/apps/nirusettings.svg"
rm -rf "$DATA/nirusettings"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true

if [[ ${1:-} == --purge ]]; then
  rm -rf "$STATE/nirusettings" "$CACHE/nirusettings"
  printf "Removed NIRUSETTINGS and its cache. Your settings files were not touched.\n"
else
  printf "Removed NIRUSETTINGS. Your settings files were not touched (--purge also removes its cache).\n"
fi
