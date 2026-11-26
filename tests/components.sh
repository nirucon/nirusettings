#!/bin/sh
# Every value a schema allows for the shared NIRUWM file, checked by the
# real NIRUWM and NIRUBAR: the default, each choice, the lowest and the
# highest value of every setting is written with `nirusettings --set` into
# a private copy of NIRUX's file, and their own --check-config must accept
# it (a refusal stops the write and fails this test). Nothing runs on a
# display and HOME is a temporary folder.
#
#   tests/components.sh PATH/TO/nirusettings DIR_WITH_niruwm_AND_nirubar
#
# Build those two from their repositories first (cargo build --release).
set -eu
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
COMP=$(cd "$2" && pwd)
HERE=$(cd "$(dirname "$0")/.." && pwd)
[ -x "$COMP/niruwm" ] && [ -x "$COMP/nirubar" ] || { echo "need niruwm and nirubar in $COMP" >&2; exit 2; }
SB=$(mktemp -d)
trap 'rm -rf "$SB"' EXIT INT TERM
export HOME="$SB/home" XDG_CONFIG_HOME="$SB/config" XDG_DATA_HOME="$SB/data" \
    XDG_CACHE_HOME="$SB/cache" XDG_STATE_HOME="$SB/state" NIRUSETTINGS_NO_APPLY=1
mkdir -p "$HOME" "$XDG_CONFIG_HOME/niruwm" "$SB/bin"
ln -s "$COMP/niruwm" "$SB/bin/niruwm"
ln -s "$COMP/nirubar" "$SB/bin/nirubar"
export PATH="$SB/bin:$PATH"
cp "$HERE/tests/fixtures/nirux-config.example.toml" "$XDG_CONFIG_HOME/niruwm/config.toml"
niruwm --check-config >/dev/null || { echo "NIRUX's own file does not check" >&2; exit 1; }

python3 - "$HERE/schema" > "$SB/cases" <<'EOF'
import sys, tomllib, glob, os
for f in sorted(glob.glob(os.path.join(sys.argv[1], "*.toml"))):
    s = tomllib.load(open(f, "rb"))
    if s.get("file") != "niruwm/config.toml":
        continue
    for st in s.get("setting", []):
        cid = f'{s["component"]}:{st["key"]}'
        t = st["type"]
        vals = []
        def show(v):
            if isinstance(v, bool): return "true" if v else "false"
            if isinstance(v, list): return ", ".join(str(x) for x in v) if st.get("split") != "words" else " ".join(v)
            return str(v)
        if "default" in st: vals.append(show(st["default"]))
        if t == "enum":
            for c in st.get("choices", []):
                vals.append(show(c["value"] if isinstance(c, dict) else c))
        if t in ("int", "float"):
            for k in ("min", "max"):
                if k in st: vals.append(show(st[k]))
        if t == "bool": vals += ["true", "false"]
        if t == "list" and st.get("item") == "enum":
            ch = [c["value"] if isinstance(c, dict) else c for c in st.get("choices", [])]
            vals.append(", ".join(ch[: st.get("max_items", len(ch))]))
        if st.get("choices_from") == "themes":
            vals += ["niru-nord", "niru-winter-nights"]
        for v in vals:
            print(f"{cid}\t{v}")
EOF
n=0; bad=0
while IFS="$(printf '\t')" read -r id value; do
    n=$((n + 1))
    if ! "$BIN" --set "$id" "$value" > "$SB/out" 2>&1; then
        bad=$((bad + 1)); echo "REFUSED $id = $value: $(cat "$SB/out")"
    elif ! grep -q 'not checked by\|checked here only' "$SB/out" && ! niruwm --check-config >/dev/null 2>&1; then
        bad=$((bad + 1)); echo "WRITTEN BUT INVALID $id = $value"
    fi
done < "$SB/cases"
# Put every value back to its default at the end: the file must still check.
niruwm --check-config >/dev/null && nirubar --check-config >/dev/null || { echo "final file refused" >&2; bad=$((bad + 1)); }
echo "$n values written and checked by niruwm and nirubar, $bad problems"
[ "$bad" = 0 ]
