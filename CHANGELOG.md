# Changelog

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
