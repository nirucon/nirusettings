#!/bin/sh
# X11 end-to-end checks in a private Xvfb with HOME and every XDG directory
# in a temporary folder, so no real configuration is ever read or written.
# NIRUX's own example files are the starting configuration; stand-ins for
# niructl, niruwm, nirubar, nirulock, nirux and the terminal log what they
# are asked (the niruwm stand-in refuses gap = 99, like a real check would
# refuse a bad value). Checked: the command line, first frame, memory, idle
# CPU, keys (step, undo, reset, toggle, search, editor, picker, the module
# list), that only the touched lines change and comments stay, the backup,
# niructl reload, a new file, extras in a terminal, a second start raising
# the first, a change made by someone else, hover, the live theme, quit.
#
#   tests/xvfb.sh PATH/TO/nirusettings [DISPLAY_NUMBER]
#
# With NIRUSETTINGS_SHOTS=DIR it also takes the README screenshots.
set -eu
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
HERE=$(cd "$(dirname "$0")/.." && pwd)
N=${2:-}
if [ -z "$N" ]; then
    N=$((70 + $$ % 20))
    while [ -e "/tmp/.X$N-lock" ]; do N=$((N + 1)); done
fi
D=:$N
SB=$(mktemp -d)
PIDS=""
cleanup() {
    if [ -n "${NIRUSETTINGS_TEST_LOG:-}" ] && [ -f "$SB/err" ]; then cp "$SB/err" "$NIRUSETTINGS_TEST_LOG"; fi
    for p in $PIDS; do kill "$p" 2>/dev/null || true; done
    rm -rf "$SB"
}
trap cleanup EXIT INT TERM
fail() { echo "FAIL: $*" >&2; grep -v 'trace|frame\|trace|fps\|trace|hover' "$SB/err" 2>/dev/null | tail -15 >&2 || true; exit 1; }

export HOME="$SB/home" XDG_CONFIG_HOME="$SB/config" XDG_DATA_HOME="$SB/data" \
    XDG_CACHE_HOME="$SB/cache" XDG_STATE_HOME="$SB/state" XDG_RUNTIME_DIR="$SB/run"
C="$XDG_CONFIG_HOME"
mkdir -p "$HOME" "$C/niruwm/themes" "$C/niruui" "$C/nirux" "$XDG_DATA_HOME" \
    "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR" "$SB/bin"
chmod 700 "$XDG_RUNTIME_DIR"
unset DBUS_SESSION_BUS_ADDRESS WAYLAND_DISPLAY NIRUSETTINGS_ROOT || true
export PATH="$SB/bin:$PATH"
F="$HERE/tests/fixtures"
cp "$F/nirux-config.example.toml" "$C/niruwm/config.toml"
sed -i 's/^theme = .*/theme = "niru-forest-vigil"/' "$C/niruwm/config.toml"
cp "$F/nirulock-config.example.toml" "$C/niruui/lock.toml"
cp "$F/nirux-sounds.example.toml" "$C/nirux/sounds.toml"
printf 'background = "101820"\nforeground = "e0e6ea"\nmuted = "8a99a6"\nborder = "2a3a48"\nfocus = "f2a65a"\n' > "$C/niruwm/themes/my-own.toml"

# Stand-ins. Each logs its arguments.
stand() { # NAME BODY
    printf '#!/bin/sh\necho "$0 $*" >> "%s/calls.log"\n%s\n' "$SB" "$2" > "$SB/bin/$1"
    chmod +x "$SB/bin/$1"
}
stand niructl 'case "$1" in
keys) echo "{\"entries\":[{\"keys\":\"super+Return\",\"label\":\"Super+Return\",\"description\":\"Open the terminal\",\"group\":\"Programs\",\"source\":\"default\"},{\"keys\":\"super+t\",\"label\":\"Super+T\",\"description\":\"Tile layout\",\"group\":\"Layouts\",\"source\":\"default\"}]}" ;;
esac; exit 0'
stand niruwm 'case "$1" in
--version) echo "NIRUWM 0.37.15" ;;
--check-config) grep -q "^gap = 99" "$XDG_CONFIG_HOME/niruwm/config.toml" && { echo "Invalid geometry or empty command" >&2; exit 1; }
  test -f "$XDG_CONFIG_HOME/niruwm/themes/my-own.toml" || { echo "themes not linked" >&2; exit 1; }
  echo "Configuration OK" ;;
esac'
stand nirubar 'case "$1" in --version) echo "NIRUBAR 0.34.2" ;; --check-config) echo "Configuration OK" ;; esac'
stand nirulock 'echo "NIRULOCK 0.29.0"'
stand niruterm 'exit 0'
stand nirux 'case "$1 $2" in
"--version ") echo "NIRUX 0.12.373" ;;
"extras list") printf "NAME         PINNED    INSTALLED    SELECTED\nnirusys      0.1.0     0.1.0        yes\nnirusid      0.1.1     -            no\nniruamp      0.6.0     0.5.0        yes\n" ;;
esac'
calls() { grep -c -- "$1" "$SB/calls.log" 2>/dev/null || true; }

Xvfb "$D" -screen 0 1600x1000x24 -nolisten tcp >/dev/null 2>&1 &
PIDS="$PIDS $!"
export DISPLAY="$D"
sleep 1

wait_for() { # PATTERN [TENTHS]
    n=0
    while ! grep -q -- "$1" "$SB/err" 2>/dev/null; do
        n=$((n + 1)); [ $n -lt "${2:-60}" ] || fail "no '$1'"; sleep 0.1
    done
}
k() { xdotool key --delay 40 "$@"; sleep 0.25; }
typ() { xdotool type --delay 30 "$1"; sleep 0.25; }
ticks() { awk '{print $14+$15}' "/proc/$1/stat"; }
cfg() { cat "$C/niruwm/config.toml"; }

# ── The command line ──────────────────────────────────────────────────
"$BIN" --version | grep -q '^NIRUSETTINGS [0-9]' || fail "--version"
"$BIN" --schemas > "$SB/schemas" || fail "--schemas"
grep -q '^niruwm ' "$SB/schemas" || fail "--schemas lists niruwm"
n=$("$BIN" --list --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')
[ "$n" -gt 150 ] || fail "only $n settings"
[ "$("$BIN" --get gap)" = 8 ] || fail "--get gap"
[ "$("$BIN" --get bar.show_title)" = false ] || fail "--get through an alias"
cp "$C/niruwm/config.toml" "$SB/before.toml"
"$BIN" --set gap 12 > "$SB/out" || fail "--set gap 12"
[ "$("$BIN" --get gap)" = 12 ] || fail "gap not 12"
# Exactly one line differs, and the comment above it is still there.
[ "$(diff "$SB/before.toml" "$C/niruwm/config.toml" | grep -c '^[<>]')" = 2 ] || fail "more than the gap line changed"
grep -q '^# Space between windows, in pixels.' "$C/niruwm/config.toml" || fail "comment lost"
cmp -s "$SB/before.toml" "$C/niruwm/config.toml.bak" || fail "no backup"
[ "$(calls 'niructl reload')" = 1 ] || fail "niructl reload not run"
grep -q 'niruwm --check-config' "$SB/calls.log" || fail "niruwm did not check"
"$BIN" --set gap 101 2>/dev/null && fail "gap 101 accepted"
"$BIN" --set gap 99 2>"$SB/out" && fail "gap 99 accepted (niruwm refuses it)"
grep -q 'niruwm refuses it' "$SB/out" || fail "refusal not reported"
[ "$("$BIN" --get gap)" = 12 ] || fail "a refused value was written"
"$BIN" --set nirulock:layout fancy 2>/dev/null && fail "unknown layout accepted"
"$BIN" --set nightlight.on 25:00 2>/dev/null && fail "25:00 accepted"
"$BIN" --set bar.show_title true >/dev/null || fail "--set via alias"
grep -q '^title = true' "$C/niruwm/config.toml" || fail "alias: title not edited in place"
grep -q 'show_title' "$C/niruwm/config.toml" && fail "alias: a second key was added"
"$BIN" --set bar.show_title false >/dev/null
echo "command line ok"

# ── The window ────────────────────────────────────────────────────────
NIRUSETTINGS_TIMING=1 NIRUSETTINGS_TRACE=1 "$BIN" 2>"$SB/err" &
A=$!
PIDS="$PIDS $A"
wait_for 'first frame after' 50
ms=$(sed -n 's/.*first frame after \([0-9.]*\) ms.*/\1/p' "$SB/err" | head -1)
[ "${ms%.*}" -lt 1000 ] || fail "first frame took $ms ms"
wait_for 'trace|version|nirulock|0.29.0'
W=$(xdotool search --pid "$A" | head -1)
[ -n "$W" ] || fail "no window"
xdotool windowmove "$W" 0 0; sleep 0.4
xdotool windowfocus --sync "$W"
echo "first frame ${ms} ms"

sleep 2
a=$(ticks $A); t0=$(date +%s%N); sleep 5; b=$(ticks $A); t1=$(date +%s%N)
rss=$(awk '/VmRSS/ {print $2}' "/proc/$A/status")
anon=$(awk '/RssAnon/ {print $2}' "/proc/$A/status")
hz=$(getconf CLK_TCK)
cpu=$(awk -v d=$((b - a)) -v hz="$hz" -v ns=$((t1 - t0)) 'BEGIN { printf "%.2f", d / hz / (ns / 1e9) * 100 }')
echo "idle: cpu ${cpu} % of one core, rss ${rss} kB (anon ${anon} kB)"
[ "$rss" -lt 40000 ] || fail "memory $rss kB"
awk -v c="$cpu" 'BEGIN { exit !(c < 0.5) }' || fail "idle cpu $cpu %"

# Window manager: the first setting is the gap (12 from the command line).
k 2
wait_for 'trace|section|Window manager'
wait_for 'trace|select|niruwm:gap'
k plus plus
wait_for 'trace|saved|niruwm:gap|14'
grep -q '^gap = 14$' "$C/niruwm/config.toml" || fail "gap 14 not written"
grep -q '^# Space between windows, in pixels.' "$C/niruwm/config.toml" || fail "comment lost (window)"
k u
wait_for 'trace|saved|niruwm:gap|13'
k r
wait_for 'trace|saved|niruwm:gap|8'
k Down space
wait_for 'trace|saved|niruwm:smart_gaps|false'
grep -q '^smart_gaps = false' "$C/niruwm/config.toml" || fail "smart_gaps not written"
echo "step, undo, reset, toggle ok"

# Search across everything; Return on a switch toggles it (another file).
k slash
typ "music while"
wait_for 'trace|search|music while|1'
k Return
wait_for 'trace|saved|nirulock:pause_music.enabled|true'
grep -A3 '^\[pause_music\]' "$C/niruui/lock.toml" | grep -q '^enabled = true' || fail "pause music not on"
k Escape
echo "search ok"

# The editor refuses what the component would refuse.
k slash
typ "not before"
k Return
wait_for 'trace|editor|niruwm:nightlight.not_before'
k ctrl+u
typ "25:00"
k Return
wait_for 'trace|invalid|niruwm:nightlight.not_before'
k Escape
grep -q '^not_before = "19:00"' "$C/niruwm/config.toml" || fail "an invalid time was written"
k Escape
echo "editor ok"

# A file that does not exist yet is created (with its header).
k slash
typ "seconds per photo"
k Return
wait_for 'trace|editor|niruscreensaver:photos_seconds'
k ctrl+u
typ "30"
k Return
wait_for 'trace|saved|niruscreensaver:photos_seconds|30'
grep -q '^photos_seconds = 30$' "$C/niruui/screensaver.toml" || fail "screensaver.toml not created"
grep -q '^# NIRUSCREENSAVER settings' "$C/niruui/screensaver.toml" || fail "no header in the new file"
k Escape
echo "new file ok"

# The theme picker: the user's own theme is a choice; the WM reloads.
reloads=$(calls 'niructl reload')
k 1
wait_for 'trace|select|niruwm:theme'
k Return
wait_for 'trace|picker|niruwm:theme'
k End Return
wait_for 'trace|saved|niruwm:theme|my-own'
grep -q '^theme = "my-own"' "$C/niruwm/config.toml" || fail "theme not written"
[ "$(calls 'niructl reload')" -gt "$reloads" ] || fail "no reload after the theme"
wait_for 'trace|theme|my-own|101820' 40
echo "theme picker and live theme ok"

# The bar's module list: switching one off keeps the order of the rest
# and stops nirux update from replacing the list.
k slash
typ "modules and"
k Return
wait_for 'trace|listed|nirubar:bar.modules'
k space
wait_for 'trace|saved|nirubar:bar.modules|dictation, privacy'
grep -q '^modules = \["dictation", "privacy"' "$C/niruwm/config.toml" || fail "modules not written"
grep -q '^follow_default_modules = false' "$C/niruwm/config.toml" || fail "follow_default_modules not cleared"
k Home shift+Down
wait_for 'trace|saved|nirubar:bar.modules|privacy, dictation'
k Escape Escape
echo "module list ok"

# Key bindings from niructl.
k 7
wait_for 'trace|keys|2|niructl keys --json'
echo "key bindings ok"

# Extras: state from nirux; installing runs in the terminal.
k 8
wait_for 'trace|extras|'
k End Up
k Return
wait_for 'trace|confirm|nirux extras install nirusid'
k Return
wait_for 'trace|terminal|niruterm -e sh -c nirux extras install nirusid'
sleep 0.3
grep -q 'niruterm -e sh -c nirux extras install nirusid' "$SB/calls.log" || fail "terminal not started"
echo "extras ok"

# Hover help.
k 2
xdotool mousemove --window "$W" 300 175; sleep 0.3
xdotool mousemove --window "$W" 302 150; sleep 0.3
wait_for 'trace|hover|'
echo "hover ok"

# A second start raises this one and opens its section.
NIRUSETTINGS_TRACE=1 "$BIN" lock 2>"$SB/err2" || fail "second start failed"
grep -q 'trace|instance|raised' "$SB/err2" || fail "second start opened a window"
wait_for 'trace|request|Lock screen'
wait_for 'trace|section|Lock screen'
echo "single instance ok"

# Someone else changes a file: the window follows.
sed -i 's/^gap = .*/gap = 20/' "$C/niruwm/config.toml"
wait_for 'trace|files|changed'
[ "$("$BIN" --get gap)" = 20 ] || fail "external change"
echo "external change ok"

if [ -n "${NIRUSETTINGS_SHOTS:-}" ]; then
    mkdir -p "$NIRUSETTINGS_SHOTS"
    shot() { # FILE
        eval "$(xwininfo -id "$W" | awk '/Absolute upper-left X:/ {print "X=" $4} /Absolute upper-left Y:/ {print "Y=" $4} /Width:/ {print "WW=" $2} /Height:/ {print "HH=" $2}')"
        sleep 0.6
        ffmpeg -loglevel error -y -f x11grab -draw_mouse 0 -video_size "${WW}x${HH}" -i "$D+$X,$Y" -frames:v 1 "$1"
    }
    xdotool windowsize "$W" 1100 720; sleep 0.5
    xdotool mousemove 1590 990
    sed -i 's/^theme = .*/theme = "niru-nord"/' "$C/niruwm/config.toml"; sleep 0.6
    k 2; k Down Down; shot "$NIRUSETTINGS_SHOTS/main.png"
    k 3; k Down Down Down Down Down Return Down Down; shot "$NIRUSETTINGS_SHOTS/modules.png"; k Escape
    sed -i 's/^theme = .*/theme = "niru-carl-larsson"/' "$C/niruwm/config.toml"; sleep 0.6
    k 4; k Down Down Down Down Down Down Down Down; shot "$NIRUSETTINGS_SHOTS/lock.png"
    sed -i 's/^theme = .*/theme = "niru-forest-vigil"/' "$C/niruwm/config.toml"; sleep 0.6
    k 1; k Return Down Down; shot "$NIRUSETTINGS_SHOTS/themes.png"; k Escape
    sed -i 's/^theme = .*/theme = "niru-blood-of-urdr"/' "$C/niruwm/config.toml"; sleep 0.6
    k slash; typ "sound"; k Down Down; shot "$NIRUSETTINGS_SHOTS/search.png"; k Escape Escape
    sed -i 's/^theme = .*/theme = "niru-winter-nights"/' "$C/niruwm/config.toml"; sleep 0.6
    k 8; k Down Down Down; shot "$NIRUSETTINGS_SHOTS/extras.png"
    echo "screenshots in $NIRUSETTINGS_SHOTS"
fi

xdotool windowfocus --sync "$W"
k ctrl+q
sleep 0.5
kill -0 "$A" 2>/dev/null && fail "Ctrl+Q did not quit"
echo PASS
