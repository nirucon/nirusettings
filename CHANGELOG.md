# Changelog

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
