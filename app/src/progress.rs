//! Progress shared between profiles (what Noita's Progress screen shows).
//!
//! Noita keeps progress inside save00 and only ever adds to it:
//!   persistent/flags/<flag>       one file per unlock / seen spell or perk
//!   persistent/orbs_new/<n>       orbs found in any run
//!   stats/stats_<enemy>.xml       kills and deaths per enemy
//!   stats/_stats.salakieli        encrypted; its KEY_VALUE_STATS list is what the Progress screen
//!                                 shows as killed enemies (and casts/picks per spell and perk)
//! Since the app swaps the whole save00, every profile would otherwise keep its own progress.
//! A pool (progress/<pool>/, same layout) holds the progress of all profiles that use it:
//! saving a profile merges its progress into the pool (only adds: unknown flags/orbs are copied,
//! enemy counters take the higher value); activating one gives it exactly the pool's progress.
//! Pools are "all", "singleplayer", "multiplayer" (from the progress mode) or a progress set the
//! user made, like separate save slots: one with everything unlocked, one for challenge runs.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::saves::{Kind, Paths};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ProgressMode {
    /// One progress for all profiles, like Noita without the app.
    #[default]
    Shared,
    /// Singleplayer profiles share one progress, multiplayer profiles (NT + EW) another.
    SplitByType,
    /// Every profile keeps its own progress.
    PerProfile,
}

impl ProgressMode {
    pub const ALL: [ProgressMode; 3] = [ProgressMode::Shared, ProgressMode::SplitByType, ProgressMode::PerProfile];

    pub fn label(self) -> &'static str {
        match self {
            ProgressMode::Shared => "Shared by all profiles",
            ProgressMode::SplitByType => "Singleplayer and multiplayer separate",
            ProgressMode::PerProfile => "Per profile",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            ProgressMode::Shared => "Every profile sees everything unlocked in any profile, like Noita without the app.",
            ProgressMode::SplitByType => "Singleplayer profiles share one progress, Noita Together and Entangled Worlds profiles another.",
            ProgressMode::PerProfile => "Each profile keeps the progress it was played with.",
        }
    }
}

/// Which progress a profile uses.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub enum ProgressChoice {
    /// Whatever the progress mode in the settings says.
    #[default]
    Default,
    /// A progress set (by id).
    Set(String),
    /// Only this profile's own progress.
    Own,
}

/// A named pool made by the user.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ProgressSet {
    pub id: String,
    pub name: String,
}

/// The pool a profile of `kind` uses by default, None when progress stays in the profile.
pub fn pool(mode: ProgressMode, kind: Kind) -> Option<&'static str> {
    match mode {
        ProgressMode::Shared => Some("all"),
        ProgressMode::SplitByType if kind.is_multiplayer() => Some("multiplayer"),
        ProgressMode::SplitByType => Some("singleplayer"),
        ProgressMode::PerProfile => None,
    }
}

/// Display name of a built-in pool.
pub fn builtin_name(pool: &str) -> Option<&'static str> {
    match pool {
        "all" => Some("Shared by all profiles"),
        "singleplayer" => Some("Singleplayer"),
        "multiplayer" => Some("Multiplayer"),
        _ => None,
    }
}

/// 2: stats/_stats.salakieli joined the progress files. A higher value merges all profiles once more.
pub const FORMAT: u32 = 2;

pub fn pool_dir(p: &Paths, pool: &str) -> PathBuf {
    p.root.join("progress").join(pool)
}

/// Save -> pool.
pub fn absorb(p: &Paths, pool: Option<&str>, save: &Path) -> Result<()> {
    match pool {
        Some(name) => merge(save, &pool_dir(p, name)),
        None => Ok(()),
    }
}

/// Pool -> save: the save gets exactly the pool's progress. A pool that doesn't exist yet is
/// started from the save instead, so the first profile never loses what it has.
pub fn apply(p: &Paths, pool: Option<&str>, save: &Path) -> Result<()> {
    let Some(name) = pool else { return Ok(()) };
    if !save.exists() {
        return Ok(());
    }
    let dir = pool_dir(p, name);
    if dir.exists() { replace(&dir, save) } else { merge(save, &dir) }
}

/// (unlocks, enemies) in a pool or save.
pub fn counts(dir: &Path) -> (usize, usize) {
    let flags = files(&dir.join("persistent/flags")).len();
    let enemies = match read_game_stats(&dir.join(GAME_STATS)).as_deref().and_then(key_values) {
        Some(kv) => kv.keys().filter(|k| !k.starts_with("action_") && !k.starts_with("perk_")).count(),
        None => files(&dir.join("stats")).iter().filter(|(n, _)| is_enemy_stats(n)).count(),
    };
    (flags, enemies)
}

/// Make the progress files of `dst` an exact copy of those in `src`; the rest of `dst` stays.
fn replace(src: &Path, dst: &Path) -> Result<()> {
    for sub in ["persistent/flags", "persistent/orbs_new"] {
        let target = dst.join(sub);
        if target.exists() {
            fs::remove_dir_all(&target).with_context(|| format!("Clear: {}", target.display()))?;
        }
        fs::create_dir_all(&target)?;
        for (name, from) in files(&src.join(sub)) {
            fs::copy(&from, target.join(name)).with_context(|| format!("Copy: {}", from.display()))?;
        }
    }
    for (name, path) in files(&dst.join("stats")) {
        if is_enemy_stats(&name) {
            fs::remove_file(&path)?;
        }
    }
    for (name, from) in files(&src.join("stats")) {
        if is_enemy_stats(&name) {
            fs::create_dir_all(dst.join("stats"))?;
            fs::copy(&from, dst.join("stats").join(name))?;
        }
    }
    // Only the list is progress; the rest of _stats (this run, best run) stays the save's own.
    if let Some(theirs) = read_game_stats(&src.join(GAME_STATS)) {
        let to = dst.join(GAME_STATS);
        match read_game_stats(&to) {
            Some(ours) => {
                if let Some(text) = key_values(&theirs).and_then(|kv| with_key_values(&ours, &kv)) {
                    write_game_stats(&to, &text)?;
                }
            }
            None if !to.exists() => write_game_stats(&to, &theirs)?,
            None => {} // unreadable: leave Noita's file alone
        }
    }
    Ok(())
}

/// Copy the progress files of `save` (persistent/, stats/stats_*.xml, stats/_stats) to `dst`, for backups.
pub fn copy_progress(save: &Path, dst: &Path) -> Result<()> {
    for sub in ["persistent/flags", "persistent/orbs_new"] {
        for (name, from) in files(&save.join(sub)) {
            let to = dst.join(sub).join(name);
            fs::create_dir_all(to.parent().unwrap())?;
            fs::copy(&from, &to)?;
        }
    }
    for (name, from) in files(&save.join("stats")) {
        if is_enemy_stats(&name) {
            fs::create_dir_all(dst.join("stats"))?;
            fs::copy(&from, dst.join("stats").join(name))?;
        }
    }
    if save.join(GAME_STATS).exists() {
        fs::create_dir_all(dst.join("stats"))?;
        fs::copy(save.join(GAME_STATS), dst.join(GAME_STATS))?;
    }
    Ok(())
}

/// Add the progress found in `src` to `dst` (both laid out like save00).
pub fn merge(src: &Path, dst: &Path) -> Result<()> {
    for sub in ["persistent/flags", "persistent/orbs_new"] {
        for (name, from) in files(&src.join(sub)) {
            let to = dst.join(sub).join(&name);
            if !to.exists() {
                fs::create_dir_all(dst.join(sub))?;
                fs::copy(&from, &to).with_context(|| format!("Copy: {}", from.display()))?;
            }
        }
    }
    for (name, from) in files(&src.join("stats")) {
        if !is_enemy_stats(&name) {
            continue;
        }
        let to = dst.join("stats").join(&name);
        let theirs = fs::read_to_string(&from).with_context(|| format!("Read: {}", from.display()))?;
        let merged = match fs::read_to_string(&to) {
            Ok(ours) => merge_stats(&ours, &theirs),
            Err(_) => Some(theirs),
        };
        if let Some(text) = merged {
            fs::create_dir_all(dst.join("stats"))?;
            fs::write(&to, text).with_context(|| format!("Write: {}", to.display()))?;
        }
    }
    if let Some(theirs) = read_game_stats(&src.join(GAME_STATS)) {
        let to = dst.join(GAME_STATS);
        match read_game_stats(&to) {
            Some(ours) => {
                if let Some(text) = merge_game_stats(&ours, &theirs) {
                    write_game_stats(&to, &text)?;
                }
            }
            None if !to.exists() => write_game_stats(&to, &theirs)?,
            None => {}
        }
    }
    Ok(())
}

fn is_enemy_stats(name: &str) -> bool {
    name.starts_with("stats_") && name.ends_with(".xml")
}

fn files(dir: &Path) -> Vec<(String, PathBuf)> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------- stats_<enemy>.xml

/// `<Stats deaths=".." kills=".." ..>` followed by maps of `<E key=".." value="..">` entries.
#[derive(PartialEq, Debug)]
struct Stats {
    attrs: Vec<(String, i64)>,
    maps: Vec<(String, BTreeMap<String, i64>)>,
}

fn tag_attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag;
    while let Some(eq) = rest.find("=\"") {
        let name = rest[..eq].rsplit(|c: char| c.is_whitespace() || c == '<').next().unwrap_or("").to_string();
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        out.push((name, after[..end].to_string()));
        rest = &after[end + 1..];
    }
    out
}

fn parse_stats(text: &str) -> Option<Stats> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let head = lines.next()?;
    if !head.starts_with("<Stats") {
        return None;
    }
    let attrs = tag_attrs(head).into_iter().map(|(k, v)| Some((k, v.parse().ok()?))).collect::<Option<Vec<_>>>()?;
    let mut maps: Vec<(String, BTreeMap<String, i64>)> = Vec::new();
    let mut open: Option<(String, BTreeMap<String, i64>)> = None;
    for l in lines {
        let closes = matches!(&open, Some((name, _)) if l == format!("</{name}>"));
        match &mut open {
            _ if l == "</Stats>" => break,
            _ if l == "</E>" => {}
            Some(_) if closes => maps.push(open.take()?),
            Some((_, map)) if l.starts_with("<E ") => {
                let a = tag_attrs(l);
                let get = |n: &str| a.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
                map.insert(get("key")?, get("value")?.parse().ok()?);
            }
            None if l.starts_with('<') && l.ends_with('>') && !l.contains(' ') && !l.starts_with("</") => {
                open = Some((l[1..l.len() - 1].to_string(), BTreeMap::new()));
            }
            _ => return None, // something we don't know: leave the file alone
        }
    }
    open.is_none().then_some(Stats { attrs, maps })
}

fn write_stats(s: &Stats, nl: &str) -> String {
    let mut out = String::from("<Stats");
    for (k, v) in &s.attrs {
        out += &format!(" {k}=\"{v}\"");
    }
    out += &format!(" >{nl}");
    for (name, map) in &s.maps {
        out += &format!("  <{name}>{nl}");
        for (k, v) in map {
            out += &format!("    <E key=\"{k}\" value=\"{v}\" >{nl}    </E>{nl}");
        }
        out += &format!("  </{name}>{nl}");
    }
    out += &format!("</Stats>{nl}");
    out
}

/// Field-wise maximum of two stats files. None when `ours` already holds everything (or a file
/// can't be read safely; then the existing one is kept).
fn merge_stats(ours: &str, theirs: &str) -> Option<String> {
    let (mut a, b) = (parse_stats(ours)?, parse_stats(theirs)?);
    let before = write_stats(&a, "\n");
    for (k, v) in b.attrs {
        match a.attrs.iter_mut().find(|(x, _)| *x == k) {
            Some((_, mine)) => *mine = (*mine).max(v),
            None => a.attrs.push((k, v)),
        }
    }
    for (name, map) in b.maps {
        let idx = match a.maps.iter().position(|(x, _)| *x == name) {
            Some(i) => i,
            None => {
                a.maps.push((name, BTreeMap::new()));
                a.maps.len() - 1
            }
        };
        let mine = &mut a.maps[idx].1;
        for (k, v) in map {
            let e = mine.entry(k).or_insert(v);
            *e = (*e).max(v);
        }
    }
    // Noita writes "\r\r\n"; keep whatever the file uses.
    let nl = if ours.contains("\r\r\n") {
        "\r\r\n"
    } else if ours.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    (write_stats(&a, "\n") != before).then(|| write_stats(&a, nl))
}

// ---------------------------------------------------------------- _stats.salakieli

const GAME_STATS: &str = "stats/_stats.salakieli";

/// Noita's .salakieli files are AES-128-CTR; each file kind has its own fixed key and IV.
fn crypt(data: &[u8]) -> Vec<u8> {
    use aes::cipher::{KeyIvInit, StreamCipher};
    let mut out = data.to_vec();
    ctr::Ctr128BE::<aes::Aes128>::new(b"SecretsOfTheAllS".into(), b"ThreeEyesAreWatc".into()).apply_keystream(&mut out);
    out
}

/// The decrypted text, None when missing or not what we expect.
fn read_game_stats(path: &Path) -> Option<String> {
    let text = String::from_utf8(crypt(&fs::read(path).ok()?)).ok()?;
    text.starts_with("<GameStats").then_some(text)
}

fn write_game_stats(path: &Path, text: &str) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, crypt(text.as_bytes())).with_context(|| format!("Write: {}", path.display()))
}

/// Byte range of the lines between <KEY_VALUE_STATS> and </KEY_VALUE_STATS>.
fn kv_range(text: &str) -> Option<std::ops::Range<usize>> {
    let open = text.find("<KEY_VALUE_STATS>")?;
    let start = open + text[open..].find('\n')? + 1;
    let close = start + text[start..].find("</KEY_VALUE_STATS>")?;
    let end = text[..close].rfind('\n').map_or(close, |i| i + 1).max(start);
    Some(start..end)
}

/// `<E key=".." value=".." >` pairs of the KEY_VALUE_STATS list; None for anything unexpected.
fn key_values(text: &str) -> Option<BTreeMap<String, i64>> {
    let mut map = BTreeMap::new();
    for l in text[kv_range(text)?].lines().map(str::trim).filter(|l| !l.is_empty()) {
        if l == "</E>" {
            continue;
        }
        if !l.starts_with("<E ") {
            return None;
        }
        let a = tag_attrs(l);
        let get = |n: &str| a.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        map.insert(get("key")?, get("value")?.parse().ok()?);
    }
    Some(map)
}

/// `text` with its KEY_VALUE_STATS list replaced by `kv`; None when nothing would change.
fn with_key_values(text: &str, kv: &BTreeMap<String, i64>) -> Option<String> {
    key_values(text)?;
    let range = kv_range(text)?;
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let list: String = kv.iter().map(|(k, v)| format!("    <E key=\"{k}\" value=\"{v}\" >{nl}    </E>{nl}")).collect();
    (text[range.clone()] != list).then(|| format!("{}{list}{}", &text[..range.start], &text[range.end..]))
}

/// Per-key maximum of both lists, written into `ours`. None when nothing changes.
fn merge_game_stats(ours: &str, theirs: &str) -> Option<String> {
    let (mut a, b) = (key_values(ours)?, key_values(theirs)?);
    for (k, v) in b {
        let e = a.entry(k).or_insert(v);
        *e = (*e).max(v);
    }
    with_key_values(ours, &a)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "<Stats deaths=\"2\" kills=\"5\" >\r\r\n  <death_map>\r\r\n    <E key=\"NULL | $damage_fire\" value=\"2\" >\r\r\n    </E>\r\r\n  </death_map>\r\r\n  <kill_map>\r\r\n    <E key=\"zombie\" value=\"5\" >\r\r\n    </E>\r\r\n  </kill_map>\r\r\n</Stats>\r\r\n";
    const B: &str = "<Stats deaths=\"1\" kills=\"9\" >\n  <death_map>\n    <E key=\"player | $damage_explosion\" value=\"3\" >\n    </E>\n  </death_map>\n  <kill_map>\n    <E key=\"zombie\" value=\"2\" >\n    </E>\n    <E key=\"bat\" value=\"4\" >\n    </E>\n  </kill_map>\n</Stats>\n";

    #[test]
    fn stats_round_trip_in_noitas_format() {
        assert_eq!(write_stats(&parse_stats(A).unwrap(), "\r\r\n"), A);
    }

    #[test]
    fn stats_merge_takes_the_higher_values() {
        let m = parse_stats(&merge_stats(A, B).unwrap()).unwrap();
        assert_eq!(m.attrs, vec![("deaths".to_string(), 2), ("kills".to_string(), 9)]);
        assert_eq!(m.maps[0].1.len(), 2);
        assert_eq!(m.maps[1].1["zombie"], 5);
        assert_eq!(m.maps[1].1["bat"], 4);
        assert!(merge_stats(&merge_stats(A, B).unwrap(), A).is_none(), "nothing new: file stays untouched");
        assert!(merge_stats(A, "<Stats kills=\"x\" >").is_none(), "unreadable input is ignored");
    }

    const G: &str = "<GameStats STATS_VERSION=\"4\" session_dead=\"0\" >\n  <KEY_VALUE_STATS>\n    <E key=\"action_bomb\" value=\"3\" >\n    </E>\n    <E key=\"zombie\" value=\"5\" >\n    </E>\n  </KEY_VALUE_STATS>\n  <session enemies_killed=\"2\" killed_by=\"x\" >\n  </session>\n</GameStats>\n";
    const H: &str = "<GameStats STATS_VERSION=\"4\" session_dead=\"1\" >\n  <KEY_VALUE_STATS>\n    <E key=\"bat\" value=\"1\" >\n    </E>\n    <E key=\"zombie\" value=\"2\" >\n    </E>\n  </KEY_VALUE_STATS>\n  <session enemies_killed=\"9\" killed_by=\"y\" >\n  </session>\n</GameStats>\n";

    #[test]
    fn game_stats_round_trip_and_merge() {
        assert_eq!(with_key_values(G, &key_values(G).unwrap()), None, "same list: unchanged");
        let m = merge_game_stats(G, H).unwrap();
        let kv = key_values(&m).unwrap();
        assert_eq!((kv["bat"], kv["zombie"], kv["action_bomb"]), (1, 5, 3));
        assert!(m.contains("killed_by=\"x\""), "the rest of the file stays ours");
        assert_eq!(merge_game_stats(&m, H), None);
        assert_eq!(crypt(&crypt(G.as_bytes())), G.as_bytes());
    }

    #[test]
    fn game_stats_follow_the_pool() {
        let t = std::env::temp_dir().join(format!("nss_gs_{}", std::process::id()));
        let (pool, save) = (t.join("pool"), t.join("save"));
        let _ = fs::remove_dir_all(&t);
        write_game_stats(&save.join(GAME_STATS), G).unwrap();
        merge(&save, &pool).unwrap();
        assert_eq!(read_game_stats(&pool.join(GAME_STATS)).unwrap(), G, "the first save starts the pool");
        write_game_stats(&t.join("other").join(GAME_STATS), H).unwrap();
        merge(&t.join("other"), &pool).unwrap();
        replace(&pool, &save).unwrap();
        let got = read_game_stats(&save.join(GAME_STATS)).unwrap();
        assert_eq!(key_values(&got).unwrap().len(), 3);
        assert!(got.contains("killed_by=\"x\""));
        assert_eq!(counts(&save).1, 2);
        let fresh = t.join("fresh");
        fs::create_dir_all(&fresh).unwrap();
        replace(&pool, &fresh).unwrap();
        assert_eq!(key_values(&read_game_stats(&fresh.join(GAME_STATS)).unwrap()).unwrap().len(), 3, "a new profile gets the list");
        fs::remove_dir_all(&t).unwrap();
    }

    /// NSS_GAME_STATS=<a _stats.salakieli>: must decrypt and survive a round trip unchanged.
    #[test]
    #[ignore]
    fn real_game_stats() {
        let text = read_game_stats(Path::new(&std::env::var("NSS_GAME_STATS").unwrap())).unwrap();
        let kv = key_values(&text).unwrap();
        assert_eq!(with_key_values(&text, &kv), None);
        eprintln!("{} entries", kv.len());
    }

    /// NSS_STATS=<a save00\stats folder>: every enemy file must parse and survive a round trip.
    #[test]
    #[ignore]
    fn real_stats_files() {
        let dir = PathBuf::from(std::env::var("NSS_STATS").unwrap());
        let mut n = 0;
        for (name, path) in files(&dir).into_iter().filter(|(n, _)| is_enemy_stats(n)) {
            let text = fs::read_to_string(&path).unwrap();
            let s = parse_stats(&text).unwrap_or_else(|| panic!("can't parse {name}"));
            assert_eq!(write_stats(&s, "\r\r\n"), text, "{name} changes on a round trip");
            n += 1;
        }
        eprintln!("{n} files ok");
    }
}
