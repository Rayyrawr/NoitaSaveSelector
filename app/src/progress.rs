//! Progress shared between profiles (what Noita's Progress screen shows).
//!
//! Noita keeps progress inside save00 and only ever adds to it:
//!   persistent/flags/<flag>       one file per unlock / seen spell or perk
//!   persistent/orbs_new/<n>       orbs found in any run
//!   stats/stats_<enemy>.xml       kills and deaths per enemy
//! Since the app swaps the whole save00, every profile would otherwise keep its own progress.
//! A pool (progress/<pool>/, same layout) collects the progress of all profiles that share it:
//! saving a profile merges its progress into the pool, activating one merges the pool into it.
//! Merging only adds: unknown flags/orbs are copied, enemy counters take the higher value.

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

/// The pool a profile of `kind` uses, None when progress stays in the profile.
pub fn pool(mode: ProgressMode, kind: Kind) -> Option<&'static str> {
    match mode {
        ProgressMode::Shared => Some("all"),
        ProgressMode::SplitByType if kind.is_multiplayer() => Some("multiplayer"),
        ProgressMode::SplitByType => Some("singleplayer"),
        ProgressMode::PerProfile => None,
    }
}

pub fn pool_dir(p: &Paths, pool: &str) -> PathBuf {
    p.root.join("progress").join(pool)
}

/// Save -> pool.
pub fn absorb(p: &Paths, mode: ProgressMode, kind: Kind, save: &Path) -> Result<()> {
    match pool(mode, kind) {
        Some(name) => merge(save, &pool_dir(p, name)),
        None => Ok(()),
    }
}

/// Pool -> save.
pub fn apply(p: &Paths, mode: ProgressMode, kind: Kind, save: &Path) -> Result<()> {
    match pool(mode, kind) {
        Some(name) if save.exists() => merge(&pool_dir(p, name), save),
        _ => Ok(()),
    }
}

/// Copy the progress files of `save` (persistent/, stats/stats_*.xml) to `dst`, for backups.
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
