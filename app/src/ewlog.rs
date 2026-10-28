//! Entangled Worlds / Noita logs: archive per profile, summarize, and watch the live proxy log.
//!
//! The proxy keeps only ew_log.txt + ew_log_old.txt next to its exe and rotates on every start,
//! so we copy them into <root>/logs/<profile-id>/ named by the log's modification time.

use crate::saves::{Paths, Settings};
use anyhow::Result;
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogKind {
    Proxy,
    Noita,
}

impl LogKind {
    pub fn label(self) -> &'static str {
        match self {
            LogKind::Proxy => "EW proxy",
            LogKind::Noita => "Noita",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub path: PathBuf,
    pub kind: LogKind,
    /// "2026-10-25_20-27-59" (local time of the last write)
    pub stamp: String,
    pub bytes: u64,
}

fn dir_of(exe: &str) -> PathBuf {
    Path::new(exe).parent().map(Path::to_path_buf).unwrap_or_default()
}

pub fn logs_dir(p: &Paths, id: &str) -> PathBuf {
    p.root.join("logs").join(id)
}

fn mtime_stamp(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Copy the current logs into the profile's log folder. Files already archived are skipped.
/// `since`: only files written after this moment (the start of the session that just ended).
pub fn archive(p: &Paths, s: &Settings, id: &str, with_proxy: bool, since: Option<SystemTime>) -> Result<usize> {
    let mut sources = vec![(dir_of(&s.noita_exe).join("logger.txt"), LogKind::Noita)];
    if with_proxy {
        let d = dir_of(&s.ew_proxy_exe);
        sources.push((d.join("ew_log.txt"), LogKind::Proxy));
        sources.push((d.join("ew_log_old.txt"), LogKind::Proxy));
    }
    let dest_dir = logs_dir(p, id);
    let mut n = 0;
    for (src, kind) in sources {
        let Ok(meta) = fs::metadata(&src) else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        if meta.len() == 0 || since.is_some_and(|t| mtime < t) {
            continue;
        }
        let tag = if kind == LogKind::Proxy { "ew" } else { "noita" };
        let dest = dest_dir.join(format!("{}_{tag}.txt", mtime_stamp(mtime)));
        if dest.exists() {
            continue;
        }
        fs::create_dir_all(&dest_dir)?;
        fs::copy(&src, &dest)?;
        n += 1;
    }
    Ok(n)
}

/// Archived sessions of a profile, newest first.
pub fn list_sessions(p: &Paths, id: &str) -> Vec<Session> {
    let mut v: Vec<Session> = fs::read_dir(logs_dir(p, id))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().into_string().ok()?;
                    let stamp = name.get(..19)?;
                    let kind = if name.ends_with("_ew.txt") { LogKind::Proxy } else { LogKind::Noita };
                    Some(Session { path: e.path(), kind, stamp: stamp.to_string(), bytes: e.metadata().map(|m| m.len()).unwrap_or(0) })
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.stamp.cmp(&a.stamp).then((a.kind == LogKind::Noita).cmp(&(b.kind == LogKind::Noita))));
    v
}

// ---------------------------------------------------------------- parsing

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// What a proxy log line means for the connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signal {
    None,
    /// Packets refused / dropped / backlog: the link to a peer is struggling.
    Congestion,
    /// A peer was kicked because the backlog lasted too long.
    Kick,
    PeerUp,
    PeerDown,
    GameClosed,
}

pub fn classify(msg: &str) -> Signal {
    if msg.contains("Kicking ") {
        Signal::Kick
    } else if msg.contains("limit exceeded")
        || msg.contains("Message dropped")
        || msg.contains("still backed up")
        || msg.contains("send buffer for peer")
        || msg.contains("Giving up on reliable")
    {
        Signal::Congestion
    } else if msg.contains("switched to connected state") || msg.contains("swithed to connected state") || msg.contains("Peer connected") {
        Signal::PeerUp
    } else if msg.contains("disconnected, freeing") || msg.contains("Peer disconnected") {
        Signal::PeerDown
    } else if msg.contains("Game closed") {
        Signal::GameClosed
    } else {
        Signal::None
    }
}

/// Peer id from "...SteamId(123)..." or "Peer connected 123".
fn peer_id(text: &str) -> Option<String> {
    if let Some(x) = text.split("SteamId(").nth(1) {
        return x.split(')').next().map(str::to_string);
    }
    text.split("Peer connected ").nth(1)?.split_whitespace().next().map(str::to_string)
}

pub struct Line {
    pub time: Option<chrono::DateTime<chrono::Local>>,
    pub level: Level,
    pub text: String,
}

/// Parse "2026-10-25T13:39:06.510916Z  WARN target: message". Non-proxy lines become Info/Error by content.
pub fn parse_line(raw: &str) -> Line {
    let mut it = raw.splitn(2, char::is_whitespace);
    let first = it.next().unwrap_or("");
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(first) {
        let rest = it.next().unwrap_or("").trim_start();
        let (lvl, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let level = match lvl {
            "WARN" => Level::Warn,
            "ERROR" => Level::Error,
            _ => Level::Info,
        };
        let text = text.trim_start();
        let text = text.strip_prefix("noita_proxy::").unwrap_or(text);
        return Line { time: Some(t.with_timezone(&chrono::Local)), level, text: text.to_string() };
    }
    let lower = raw.to_lowercase();
    let level = if lower.contains("error") || lower.contains("exception") {
        Level::Error
    } else if lower.contains("warning") {
        Level::Warn
    } else {
        Level::Info
    };
    Line { time: None, level, text: raw.to_string() }
}

#[derive(Clone, Debug)]
pub struct Incident {
    pub from: String,
    pub to: String,
    pub congestion: usize,
    pub kicks: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub start: Option<String>,
    pub end: Option<String>,
    pub duration: Option<String>,
    pub lines: usize,
    pub warns: usize,
    pub errors: usize,
    pub kicks: usize,
    pub game_closed: usize,
    pub peers: usize,
    pub incidents: Vec<Incident>,
}

/// A collapsed view line: same message (ignoring numbers) in a row is shown once with a count.
pub struct ViewLine {
    pub time: String,
    pub level: Level,
    pub text: String,
    pub repeat: usize,
}

pub struct Loaded {
    pub summary: Summary,
    pub all: Vec<ViewLine>,
    pub problems: Vec<ViewLine>,
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_ascii_digit()).collect()
}

/// Folds repeats into a recent line of the same minute, so alternating bursts (A B A B …) collapse too.
fn push_collapsed(v: &mut Vec<ViewLine>, l: &Line) {
    let time = l.time.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default();
    let key = norm(&l.text);
    let start = v.len().saturating_sub(6);
    if let Some(prev) = v[start..].iter_mut().rev().find(|p| p.level == l.level && p.time.get(..5) == time.get(..5) && norm(&p.text) == key) {
        prev.repeat += 1;
        return;
    }
    v.push(ViewLine {
        time,
        level: l.level,
        text: l.text.chars().take(400).collect(),
        repeat: 1,
    });
}

pub fn load(path: &Path) -> Result<Loaded> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut s = Summary::default();
    let mut all = Vec::new();
    let mut problems = Vec::new();
    let mut first_t = None;
    let mut last_t = None;
    let mut peers = BTreeSet::new();
    let mut is_host = false;
    let mut cur: Option<(chrono::DateTime<chrono::Local>, chrono::DateTime<chrono::Local>, usize, usize)> = None;
    for raw in text.lines() {
        if raw.trim().is_empty() {
            continue;
        }
        let l = parse_line(raw);
        s.lines += 1;
        match l.level {
            Level::Warn => s.warns += 1,
            Level::Error => s.errors += 1,
            Level::Info => {}
        }
        if let Some(t) = l.time {
            first_t.get_or_insert(t);
            last_t = Some(t);
        }
        is_host |= l.text.contains("Is host: true");
        let sig = classify(&l.text);
        match sig {
            Signal::Kick => s.kicks += 1,
            Signal::GameClosed => s.game_closed += 1,
            Signal::PeerUp => {
                // In IP mode the host logs itself as peer 0.
                if let Some(id) = peer_id(&l.text).filter(|id| !(is_host && id == "0")) {
                    peers.insert(id);
                }
            }
            _ => {}
        }
        if matches!(sig, Signal::Congestion | Signal::Kick) {
            if let Some(t) = l.time {
                let k = (sig == Signal::Kick) as usize;
                let c = (sig == Signal::Congestion) as usize;
                cur = match cur {
                    Some((from, to, cc, kk)) if (t - to).num_seconds() <= 60 => Some((from, t, cc + c, kk + k)),
                    Some(done) => {
                        s.incidents.push(incident(done));
                        Some((t, t, c, k))
                    }
                    None => Some((t, t, c, k)),
                };
            }
        }
        push_collapsed(&mut all, &l);
        if l.level != Level::Info || sig != Signal::None {
            push_collapsed(&mut problems, &l);
        }
    }
    if let Some(done) = cur {
        s.incidents.push(incident(done));
    }
    s.peers = peers.len();
    if let (Some(a), Some(b)) = (first_t, last_t) {
        s.start = Some(a.format("%Y-%m-%d %H:%M").to_string());
        s.end = Some(b.format("%H:%M").to_string());
        let m = (b - a).num_minutes();
        s.duration = Some(if m >= 60 { format!("{} h {:02} min", m / 60, m % 60) } else { format!("{m} min") });
    }
    Ok(Loaded { summary: s, all, problems })
}

fn incident((from, to, c, k): (chrono::DateTime<chrono::Local>, chrono::DateTime<chrono::Local>, usize, usize)) -> Incident {
    Incident { from: from.format("%H:%M:%S").to_string(), to: to.format("%H:%M:%S").to_string(), congestion: c, kicks: k }
}

// ---------------------------------------------------------------- live watch

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetState {
    Waiting,
    Good,
    Shaky,
    Kicked,
}

impl NetState {
    pub fn label(self) -> &'static str {
        match self {
            NetState::Waiting => "waiting for players",
            NetState::Good => "stable",
            NetState::Shaky => "shaky",
            NetState::Kicked => "dropped / kicked",
        }
    }
}

/// Tails the proxy's ew_log.txt while it runs.
pub struct NetWatch {
    path: PathBuf,
    offset: u64,
    rest: String,
    congestion: VecDeque<Instant>,
    last_kick: Option<Instant>,
    peers: BTreeSet<String>,
    is_host: bool,
    pub incidents: usize,
    in_incident: bool,
}

impl NetWatch {
    pub fn new(s: &Settings) -> Self {
        NetWatch {
            path: dir_of(&s.ew_proxy_exe).join("ew_log.txt"),
            offset: 0,
            rest: String::new(),
            congestion: VecDeque::new(),
            last_kick: None,
            peers: BTreeSet::new(),
            is_host: false,
            incidents: 0,
            in_incident: false,
        }
    }

    pub fn poll(&mut self) {
        let Ok(mut f) = fs::File::open(&self.path) else { return };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // Proxy restarted and rotated the log.
            let path = std::mem::take(&mut self.path);
            *self = NetWatch { path, ..NetWatch::new(&Settings::default()) };
        }
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if f.take(4 * 1024 * 1024).read_to_end(&mut buf).is_err() {
            return;
        }
        self.offset += buf.len() as u64;
        self.rest.push_str(&String::from_utf8_lossy(&buf));
        let now = Instant::now();
        let complete = match self.rest.rfind('\n') {
            Some(i) => {
                let tail = self.rest.split_off(i + 1);
                std::mem::replace(&mut self.rest, tail)
            }
            None => return,
        };
        for raw in complete.lines() {
            let l = parse_line(raw);
            self.is_host |= l.text.contains("Is host: true");
            match classify(&l.text) {
                Signal::Congestion => self.congestion.push_back(now),
                Signal::Kick => self.last_kick = Some(now),
                Signal::PeerUp => {
                    if let Some(id) = peer_id(&l.text).filter(|id| !(self.is_host && id == "0")) {
                        self.peers.insert(id);
                    }
                }
                Signal::PeerDown => {
                    if let Some(id) = l.text.split_whitespace().find(|w| w.chars().all(|c| c.is_ascii_digit()) && w.len() > 8) {
                        self.peers.remove(id);
                    }
                }
                _ => {}
            }
        }
        while self.congestion.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(20)) {
            self.congestion.pop_front();
        }
        let bad = self.state() != NetState::Good && self.state() != NetState::Waiting;
        if bad && !self.in_incident {
            self.incidents += 1;
        }
        self.in_incident = bad;
    }

    pub fn peers(&self) -> usize {
        self.peers.len()
    }

    pub fn state(&self) -> NetState {
        if self.last_kick.is_some_and(|t| t.elapsed() < Duration::from_secs(90)) {
            NetState::Kicked
        } else if self.congestion.len() >= 20 {
            NetState::Shaky
        } else if self.peers.is_empty() {
            NetState::Waiting
        } else {
            NetState::Good
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
2026-10-25T12:15:16.447628Z  INFO noita_proxy::net::steam_networking: Connection of peer SteamId(76561190000000001) swithed to connected state
2026-10-25T13:39:06.510916Z  WARN noita_proxy::net::steam_networking: Steam's send buffer for peer SteamId(76561190000000001) is full; holding reliable messages to it until there is room
2026-10-25T13:39:06.631285Z  WARN noita_proxy::net: Error while sending message of len 13: Message dropped
2026-10-25T13:39:06.712100Z  WARN noita_proxy::net: Error while sending message of len 14: Message dropped
2026-10-25T13:39:36.515544Z ERROR noita_proxy::net: Kicking 76561190000000001: our reliable messages to them have been backed up for too long
2026-10-25T16:59:56.731397Z  WARN noita_proxy::net: Game closed (Lost connection to noita instance: Couldn't receive the length from stream)
2026-10-25T17:10:00.000000Z  INFO noita_proxy::net: Message dropped
";

    /// `NSS_LOG=<file> cargo test --release -- --ignored real_log --nocapture`
    #[test]
    #[ignore]
    fn real_log() {
        let f = PathBuf::from(std::env::var("NSS_LOG").unwrap());
        let t = Instant::now();
        let l = load(&f).unwrap();
        eprintln!("parsed in {:?}: {:#?}", t.elapsed(), l.summary);
        eprintln!("lines all={} problems={}", l.all.len(), l.problems.len());
        for v in l.problems.iter().take(12) {
            eprintln!("{} x{} {:?} {}", v.time, v.repeat, v.level, v.text.chars().take(90).collect::<String>());
        }
    }

    #[test]
    fn summary_finds_incidents_and_collapses() {
        let dir = std::env::temp_dir().join(format!("nss_log_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x_ew.txt");
        fs::write(&f, LOG).unwrap();
        let l = load(&f).unwrap();
        assert_eq!(l.summary.kicks, 1);
        assert_eq!(l.summary.peers, 1);
        assert_eq!(l.summary.game_closed, 1);
        assert_eq!(l.summary.incidents.len(), 2, "13:39 cluster and the lone 17:10 line");
        assert_eq!(l.summary.incidents[0].kicks, 1);
        let dropped = l.all.iter().find(|v| v.text.contains("Message dropped")).unwrap();
        assert_eq!(dropped.repeat, 2);
        let _ = fs::remove_dir_all(dir);
    }
}
