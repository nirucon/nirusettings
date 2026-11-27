# Changelog

# NIRUSETTINGS 0.2.1

- Bar defaults follow NIRUWM 0.37.18 and NIRUBAR 0.35: the window title is
  on, quick panels close after 6 seconds, and the default module list has
  `weather` before the clock instead of `agent`, `dnd` and `quicksettings`.

# NIRUSETTINGS 0.2.0

- Lock screen, Layout has `hacker` (the NIRUX intrusion console, NIRULOCK 0.34).
- **Scratchpads 1 to 9**: Window manager, Scratchpads shows the default
  scratchpad and all nine slots with their keys (Super+Alt+N shows or hides
  one, Super+Alt+Shift+N sends a window there), what each holds, its
  program, window class and size. A program is picked from the installed
  ones (the XDG desktop files, with icons) or typed, and the class NIRUWM
  needs is filled in: the program's own class option when it has one
  (a terminal program runs in the terminal as
  `niruterm --class scratch-btop -e btop`), else
  `sh -c "exec PROGRAM" --class CLASS` with the real class from the desktop
  file's StartupWMClass or the list of NIRUX programs; a guessed class is
  marked, and the class can be typed. Delete clears a slot.
- **Programs at login** (`[[autostart.apps]]`): workspace, program and the
  class autostart waits for; add from the installed programs, edit, remove,
  reorder (Shift+Up/Down).
- **Window rules** (`[[rules]]`): class, workspace, floating, centred, on
  every workspace and opacity, per program, in order (the first match wins).
  Workspaces are shown and typed from 1 and written from 0, as NIRUWM reads
  them; NIRUWM's warning about a rule one workspace off is shown.
- **Key bindings can be changed**: change the keys (type them, or press the
  combination) or the action (picked from every NIRUWM action), add one,
  remove one (a built-in binding is released with `unbind`), or put
  NIRUWM's own back with `r`. Saving on a key that is already bound asks
  first; a binding that is the same physical key as another on the current
  keyboard layout (Super+/ and Super+7 on a Swedish keyboard) is marked and
  explained, as NIRUWM warns. NIRUWM's built-in table comes from
  `niruwm --dump-config`, and `niruwm --check-config` checks every change.
- **Screensaver preview**: `p` on the scene, in the scene picker or in the
  shuffle list runs `niruscreensaver --preview SCENE`, and a Preview row
  follows the scene.
- **NIRUBAR's own file** (`~/.config/nirubar/config.toml`): the weather
  place and refresh, the focus timer (length, sound, Do Not Disturb), the
  calendar (refresh, days ahead, next event in the bar) and how long each
  of the 39 panels stays open: 49 settings, 273 in all.
- The schema format has lists of tables (`[[table]]` with
  `[[table.field]]`, documented in docs/SCHEMA.md) and a `preview` command
  for a setting. A list is edited one entry at a time in a form and checked
  as a whole before it is written; entries nobody touched keep their
  comments and place, and a save the component refuses brings the form back.
- The command line takes lists too: `--get` (one inline table per line),
  `--set ID '[{...}]'`, `--add ID FIELD=VALUE...`, `--remove ID WHICH` and
  `--reset ID`; `--list` shows them.
- What a checker accepts but warns about is shown after the save.
- The scratchpad settings have a group of their own.

# NIRUSETTINGS 0.1.1

- The new lock screens and screensaver scenes can be picked: Lock screen,
  Layout has `attract` (NIRUX RAID, NIRULOCK 0.31), `satie` (Erik Satie
  loading from tape, 0.32) and `bbs` (NIRUX BBS, 0.33); Screensaver, Scene
  and the shuffle list have `c64` and `c64-sid` (NIRUSCREENSAVER 0.19). A
  choice newer than the installed program is refused, as before.
- Every schema checked against the current programs: 224 settings (was
  168). New: the lock screen and screensaver `[palette]` colours and the
  screensaver rune font; appearance colours (surface, selected, success,
  disabled), font size and path, spacing and focus width; the meeting,
  volume limit and dictation sounds and the notification sound exclusions;
  bar tags, battery, network, wifi and weather position; window manager
  brightness device, swallowing lists, autostart, scratchpad command,
  studio mode and battery and power settings; NIRUSETTINGS in Extras.
- An empty power profile (`""`, the same as keep) is no longer shown as
  invalid.
- A new file's header comment stays above the first setting written to it
  (it ended up below).

# NIRUSETTINGS 0.1.0

The first release: the settings of the NIRUX desktop in one retro window,
as an extra.

- 168 settings in six sections, generated from schema files written from
  the components' own parsers: Appearance (theme with swatches, fonts,
  wallpaper, reduced motion, colour overrides), Window manager (gaps,
  borders, master share, workspaces and start layouts, swallowing,
  scratchpad, picture in picture, programs, night light), Bar (modules and
  their order, edge, height, transparency, presenting, labels, title,
  clock), Lock screen (NIRULOCK 0.30: the demo layout, pause music on and
  shuffled), Screensaver (the NIRUIDLE timers and NIRUSCREENSAVER) and Sound
  (the NIRUX event sounds).
- Key bindings (read only, searchable), Extras (install, update and remove
  in a terminal) and Updates (versions, `nirux update` in a terminal).
- Search across everything, a modified marker, the source of every value
  (file or default), reset, undo, hover help, keyboard and mouse.
- Safe writes: schema checks, `toml_edit` edits that keep comments and
  order, the real `niruwm --check-config` and `nirubar --check-config` on a
  candidate before anything is written, a backup once per session, atomic
  replacement, and changes by others kept and shown.
- Applied live with `niructl reload` (NIRUWM, NIRUBAR, the appearance);
  NIRUIDLE and the sounds reread their files; the lock screen and the
  screensaver read theirs at the next start, and the window says so.
- The command line: `--list [--json]`, `--get`, `--set`, `--reset`,
  `--schemas`; one window (a second start raises it and opens a section).
- The schema format is documented in docs/SCHEMA.md and the files are
  installed for NIRULAUNCHER to read.
- First frame 12 to 30 ms, about 6 MB resident, no CPU while idle.
