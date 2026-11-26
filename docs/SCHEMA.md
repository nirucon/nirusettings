# The NIRUSETTINGS schema format

Every setting NIRUSETTINGS shows is described in a schema file: one TOML
file per component, in [`schema/`](../schema). The window, the search, the
checks and `nirusettings --list` are all generated from these files, so a
new setting is a schema edit and nothing else. Other programs (NIRULAUNCHER)
can read the same files: `install.sh` puts them in
`~/.local/share/nirusettings/schema/`.

This is format 1.

## Where the files are read from

1. `$NIRUSETTINGS_SCHEMA_DIR/NAME.toml`, when that variable is set, else
   `$XDG_DATA_HOME/nirusettings/schema/NAME.toml`
   (`~/.local/share/nirusettings/schema/`). `install.sh` writes the files of
   the installed version there, and replaces them at every install.
2. The copy built into the binary, when the file above is missing or has a
   problem. `nirusettings --schemas` lists the files in use and the problems.

The built-in files are `niruwm.toml`, `nirux-appearance.toml`,
`nirubar.toml`, `nirulock.toml`, `niruidle.toml`, `niruscreensaver.toml`
and `nirux-sounds.toml`, in the order their settings are listed.
`extras.toml` describes the extras for the Extras view.

## A schema file

```toml
format = 1                         # this format
component = "nirulock"             # a short id, used in setting ids
name = "NIRULOCK"                  # shown in the window
file = "niruui/lock.toml"          # relative to $XDG_CONFIG_HOME (~/.config)
new_file = "# NIRULOCK settings\n" # the start of the file when it is created
binary = "nirulock"                # the program (for its --version)
check = [["niruwm", "--check-config"]]  # optional checkers, see below
check_links = ["nirubar"]          # more paths the checkers read
apply = "next"                     # the default apply mode of the settings

[modes.next]                       # one table per apply mode
says = "takes effect the next time the screen locks"
live = false                       # true: the change is visible at once
# run = ["niructl", "reload"]      # optional: run after every write

[[setting]]
key = "pause_music.track_seconds"  # dotted path in the file
type = "int"
min = 5
max = 3600
step = 10                          # what + and - add
unit = "s"
default = 180                      # what the component uses when the key is absent
section = "Lock screen"
group = "Pause music"
label = "Seconds per SID tune"
help = "Unless the HVSC Songlengths.md5 knows the length."
```

Several schema files may describe the same configuration file (NIRUWM,
NIRUBAR and NIRUIDLE all live in `niruwm/config.toml`). Their checkers are
all run for every change to that file.

## Setting keys

| Key | Meaning |
|---|---|
| `key` | The dotted key path: `gap`, `bar.position`, `sounds.system.lock`. A setting's id is `component:key`. |
| `aliases` | Other names the component accepts for the same key in the same table (`["title"]` for `bar.show_title`). A change is written to the name the file already uses, never as a second key. |
| `type` | `bool`, `int`, `float`, `enum`, `string`, `path`, `colour` (6 hex digits) or `list`. |
| `label`, `help` | The name in the list and one line of help (shown under the list and on hover). |
| `section` | One of `Appearance`, `Window manager`, `Bar`, `Lock screen`, `Screensaver`, `Sound`. |
| `group` | A heading inside the section. |
| `default` | The value the component uses when the key is absent. Required unless `optional`. It is checked against the setting's own rules. |
| `optional` | The key may be absent with no default ("unset"). Clearing the value removes the key. |
| `min`, `max`, `step`, `unit` | Numbers: the allowed range, what + and - change, and a unit shown after the value. |
| `choices` | Enums and lists of enums: a list of values (`["top", "bottom"]`) or of tables `{ value = 1, label = "numbers", help = "..." }`. Values may be strings or numbers. |
| `choices_from` | Choices read at run time instead: `themes` (NIRUWM's built-in themes and `~/.config/niruwm/themes/*.toml`). |
| `item` | Lists: `string` (the default) or `enum` (each item one of `choices`). |
| `unique`, `ordered` | Lists: every item at most once; the order matters. A unique list of enum items gets the list editor (Space switches an item, Shift+Up/Down moves it). |
| `min_items`, `max_items` | Lists: how many items. |
| `split` | Lists typed as text: `comma` (`clock, photos`) or `words` (a command line, with "double quotes" around a word with spaces; never run by a shell). |
| `pattern` | A named check of text: `nonempty`, `hhmm` (07:30), `hex`, `btaddr` (AA:BB:CC:DD:EE:FF or empty), `strftime` (a date format the bar can print), `theme` (a NIRUWM theme name), `word`. |
| `apply` | The apply mode of this setting, when not the file's default. |
| `since` | The version of the component that first reads the key (`"0.30"`). The window marks the setting while an older version is installed; older versions ignore unknown keys. |
| `also_set` | Other keys written with this one, every time: `{ "bar.follow_default_modules" = false }` keeps `nirux update` from replacing a module list you arranged. |
| `no_reset` | No reset to the default (NIRUX writes a better value than the component's built-in default, such as its own terminal). |
| `read_only` | Shown, never changed. |

## Checking and writing

A change is checked three times before the file is replaced:

1. Against the schema (type, range, choices, pattern, list rules). The
   window and `--set` refuse anything else.
2. The edit itself: the file is parsed with `toml_edit`, only the keys
   involved change (comments, order and every other line stay), and the
   result must parse and hold the new value.
3. By the components, when installed: each `check` command runs with
   `XDG_CONFIG_HOME` pointing at a temporary folder that holds the
   candidate file at its place, the real files next to it (linked) and the
   `check_links`. Exit status 0 means accepted. NIRUWM and NIRUBAR refuse a
   whole file for one unknown key or bad value, so their own
   `--check-config` decides.

Then the old file is kept once per session as `FILE.bak`, the new one is
written to a temporary name, flushed and renamed over the old one (its
permissions are kept), and the apply mode's `run` command is started.

Every write starts again from the file on disk, so a change somebody else
made meanwhile (NIRUWM saves layouts into its file) is kept.

## Tests

`tests/schemas.rs` checks every key of every schema against the
components' example configurations (copies in `tests/fixtures/`, and the
component repositories when they are checked out next to this one): the key
must exist there, a value set there must pass the schema's checks, and the
default must be what the example sets (a short list of deliberate
differences is named in the test).
