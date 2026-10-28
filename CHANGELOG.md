# Changelog

## 1.2.1 – 2026-10-28
- "Keep progress" on a new profile now also carries over the enemies in Noita's Progress screen (stats/), not only the unlocked spells and perks. Before, every new profile started with an empty enemy list

## 1.2.0 – 2026-10-28
- New "Import current run" button: a run started directly in Noita (e.g. a daily) becomes its own profile; the previously active profile keeps its run
- The app now recognizes which run a save holds. If Noita holds a different run than the active profile, it no longer saves automatically, and switching or "Save now" ask first (import as new profile, or overwrite the profile)
- Header shows "In game: a different run" plus a banner in that case

## 1.1.2 – 2026-10-28
- After a self-update the app tells Windows to refresh icons, so desktop shortcuts show the new icon right away

## 1.1.1 – 2026-10-28
- Settings no longer jitter when the window is small: dialogs have a fixed width, the settings body scrolls and Save/Cancel stay visible
- Settings are grouped into Programs, Behavior, About and Uninstall
- New "Uninstall" button in Settings. The save that is loaded in Noita stays where it is; optionally the stored profiles, snapshots and logs are deleted too
- Installer: `/DATADIR=<folder>` for scripted installs

## 1.1.0 – 2026-10-28
- The whole app and the installer are now in English
- New icon: a perk-style tile with a floppy disk
- Removed the Classic theme; there is one layout now
- Dates are shown as YYYY-MM-DD

## 1.0.1 – 2026-10-28
- Update dialog: replaced a missing arrow glyph
- Release notes only list the changes of that version

## 1.0.0 – 2026-10-28
- Installer (Inno Setup, no admin rights needed) with a selectable data folder
- Automatic path detection: Steam (registry + all libraries), Noita, Noita Together, Entangled Worlds proxy
- Patcher: checks GitHub Releases, downloads with SHA-256 verification, swaps the exe and restarts
- Only one instance at a time (protects the live save)
- Crash log `crash.log` in the data folder
- App icon
