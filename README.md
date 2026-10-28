# Noita Save Selector

Profile für Noita-Spielstände: Singleplayer, **Noita Together** und **Entangled Worlds** nebeneinander, ohne dass sich die Runs gegenseitig überschreiben. Ein Klick tauscht den Spielstand und startet das passende Programm.

Windows, eine einzelne exe, keine Admin-Rechte nötig.

## Installation
1. Neueste `NoitaSaveSelector-Setup-x.y.z.exe` unter [Releases](https://github.com/Rayyrawr/NoitaSaveSelector/releases) laden und starten.
2. Daten-Ordner wählen (Standard `%LOCALAPPDATA%\NoitaSaveSelector`). Dort liegen Profile, Snapshots und Logs.
3. Der Installer sucht selbst nach **Steam**, **Noita**, **Noita Together** und dem **Entangled-Worlds-Proxy** (`noita_proxy.exe`) und zeigt am Ende, was er gefunden hat.

Fehlt etwas, in der App unter *Einstellungen* den Pfad eintragen oder *Pfade automatisch suchen* klicken.

**Portabel:** Statt des Installers geht auch nur `NoitaSaveSelector.exe` in einen eigenen Ordner, daneben eine leere Datei `portable.txt`. Dann bleibt alles in diesem Ordner.

## Updates (Patcher)
Die App fragt beim Start GitHub nach einer neueren Version (abschaltbar). Gibt es eine, erscheint oben **Update x.y.z**.
*Jetzt aktualisieren* lädt die neue exe, prüft die SHA-256-Prüfsumme, tauscht sie aus und startet neu. Profile und Einstellungen bleiben unverändert.
Manuell: *Einstellungen > Nach Updates suchen*. Alternativ einfach den neuen Installer über die alte Version installieren.

## Erster Start
Die App erkennt, dass der aktuelle Spielstand noch zu keinem Profil gehört, und bietet an, ihn zu übernehmen.
Der Typ (Entangled Worlds / Noita Together / Singleplayer) wird aus den aktiven Mods erkannt. Am Spielstand selbst ändert sich dabei nichts.

## Bedienung
- **Profil auswählen** → *Aktivieren & Spielen*.
  - Singleplayer: startet `noita.exe`
  - Entangled Worlds: startet den EW-Proxy (Steam wird bei Bedarf gestartet), der dann Noita startet
  - Noita Together: startet die Noita-Together-App + Noita
- **Wechseln geht nur, wenn Noita, EW-Proxy und Noita Together geschlossen sind.**
- Nach Spielende sichert die App den Spielstand automatisch ins aktive Profil (abschaltbar).
- *+ Neues Profil* startet einen frischen Run. Mod-Liste und Freischaltungen werden übernommen, die MP-Mods passend zum Typ an- oder ausgeschaltet.
- Oben rechts zwischen **Modern** und **Classic** umschalten.

## Was wird getauscht?
| | Ort |
|---|---|
| Noita-Spielstand (+ Mod-Liste) | `%USERPROFILE%\AppData\LocalLow\Nolla_Games_Noita\save00` |
| Entangled-Worlds-Run (nur EW-Profile) | `%APPDATA%\quant\entangledworlds\data\save_state` |

`save_shared` und die EW-Einstellungen bleiben unangetastet. Freischaltungen (`save00\persistent`) gehören zum jeweiligen Profil.

## Sicherheit
- Der Live-Ordner enthält eine Markierung `.nss_profile`. Zurück ins Profil geschrieben wird **nur**, wenn sie passt. Wurde der Ordner von außen ausgetauscht (z.B. durch Steam Cloud), verweigert die App das Sichern.
- Vor jedem Sichern landet die alte Profilkopie als Snapshot in `backups\<profil-id>\` (Anzahl einstellbar, Standard 8).
- `backups\_last_live`: Live-Stand direkt vor dem letzten Wechsel. `backups\_unassigned`: Daten, die keinem Profil gehörten (wird nie gelöscht). `backups\_deleted`: gelöschte Profile.
- Es läuft immer nur eine Instanz der App.
- Deinstallieren löscht **keine** Profile oder Backups.
- **Steam Cloud für Noita ausschalten** (die App zeigt den Status oben an): Steam → Noita → Eigenschaften → Allgemein → „Spielstände in der Steam Cloud speichern“.

## Logs & Netz (Entangled Worlds)
- Nach jeder Session kopiert die App `ew_log.txt` (EW-Proxy) und `logger.txt` (Noita) nach `logs\<profil-id>\`.
- Tab **Logs**: Sessions pro Profil, Auswertung (Dauer, Mitspieler, Störungen, Kicks) und Logzeilen.
- Während der Proxy läuft, zeigt die Statusleiste **Netz: stabil / wackelt / Abbruch**.

## Direkt-IP (z.B. Tailscale)
Im EW-Profil unter *Verbindung* auf **Direkt-IP** umschalten: *Per IP hosten* (`--host <port>`) oder *Beitreten* mit `IP:Port`. Danach oben **Proxy beenden** (im IP-Modus hat der Proxy kein Fenster).

## Bauen
Voraussetzungen: Rust (MSVC), [Inno Setup 6](https://jrsoftware.org/isinfo.php) für den Installer.

```powershell
cd app; cargo test; cd ..
.\installer\build.ps1      # -> dist\NoitaSaveSelector-Setup-<version>.exe, dist\NoitaSaveSelector.exe(.sha256)
```

**Release:** Version in `app\Cargo.toml` erhöhen, `CHANGELOG.md` ergänzen, Tag `vX.Y.Z` pushen. GitHub Actions baut, testet und veröffentlicht Installer + exe + Prüfsumme; der Patcher in der App findet das Release dann automatisch.

Tests gegen eine Kopie echter Daten: `NSS_ROOT`, `NSS_LIVE_SAVE`, `NSS_EW_STATE` auf Sandbox-Ordner setzen (siehe `saves.rs`).

## Lizenz
MIT
