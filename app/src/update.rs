//! Patcher: checks GitHub Releases for a newer version and swaps the exe in place.
//!
//! A release carries `NoitaSaveSelector.exe` and `NoitaSaveSelector.exe.sha256`. The running exe is
//! renamed to `.old.exe` (Windows allows renaming a running exe), the verified download takes its
//! place, and the new version is started with `--updated`. The old file is removed on the next start.

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REPO: &str = "Rayyrawr/NoitaSaveSelector";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const ASSET: &str = "NoitaSaveSelector.exe";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    exe_url: String,
    sha_url: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout_read(Duration::from_secs(60)).build()
}

/// "v1.2.3" / "1.2" -> [1, 2, 3] / [1, 2, 0]
fn parse_version(v: &str) -> [u64; 3] {
    let mut out = [0; 3];
    let core = v.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next().unwrap_or_default();
    for (slot, part) in out.iter_mut().zip(core.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    out
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    parse_version(candidate) > parse_version(current)
}

/// The latest release if it is newer than this build, otherwise None.
pub fn check() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = agent()
        .get(&url)
        .set("User-Agent", &format!("NoitaSaveSelector/{VERSION}"))
        .set("Accept", "application/vnd.github+json")
        .call();
    let body = match resp {
        Ok(r) => r.into_string()?,
        Err(ureq::Error::Status(404, _)) => return Ok(None), // no release published yet
        Err(e) => return Err(anyhow!("GitHub not reachable: {e}")),
    };
    let json: serde_json::Value = serde_json::from_str(&body)?;
    let tag = json["tag_name"].as_str().unwrap_or_default().to_string();
    if !is_newer(&tag, VERSION) {
        return Ok(None);
    }
    let assets = json["assets"].as_array().cloned().unwrap_or_default();
    let url_of = |name: &str| {
        assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))).and_then(|a| a["browser_download_url"].as_str()).map(str::to_string)
    };
    let exe_url = url_of(ASSET).ok_or_else(|| anyhow!("Release {tag} has no {ASSET}"))?;
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: json["body"].as_str().unwrap_or_default().trim().to_string(),
        page: json["html_url"].as_str().unwrap_or_default().to_string(),
        exe_url,
        sha_url: url_of(&format!("{ASSET}.sha256")),
    }))
}

fn download(url: &str, limit: u64) -> Result<Vec<u8>> {
    let resp = agent().get(url).set("User-Agent", &format!("NoitaSaveSelector/{VERSION}")).call().with_context(|| format!("Download failed: {url}"))?;
    let mut buf = Vec::new();
    resp.into_reader().take(limit).read_to_end(&mut buf)?;
    Ok(buf)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn old_exe(exe: &Path) -> PathBuf {
    exe.with_extension("old.exe")
}

/// Download, verify and put the new exe in place. Does not restart.
pub fn install(r: &Release) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let data = download(&r.exe_url, 100 * 1024 * 1024)?;
    if data.len() < 100_000 || &data[..2] != b"MZ" {
        bail!("Download is not a valid exe ({} bytes)", data.len());
    }
    let sha_url = r.sha_url.as_deref().ok_or_else(|| anyhow!("Release has no checksum – update aborted"))?;
    let sha_text = String::from_utf8_lossy(&download(sha_url, 4096)?).to_lowercase();
    let expected = sha_text.split_whitespace().next().unwrap_or_default().to_string();
    let actual = hex(&Sha256::digest(&data));
    if expected != actual {
        bail!("Checksum mismatch (expected {expected}, got {actual}) – update aborted");
    }

    let new = exe.with_extension("update.exe");
    let old = old_exe(&exe);
    std::fs::write(&new, &data).with_context(|| format!("Write failed: {}", new.display()))?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).context("Could not rename the running exe")?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e).context("Could not put the new exe in place");
    }
    Ok(exe)
}

/// Leftovers of a previous update.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_exe(&exe));
        let _ = std::fs::remove_file(exe.with_extension("update.exe"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("v1.0.1", "1.0.0"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(is_newer("v2", "1.99.99"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("v0.9.0", "1.0.0"));
        assert!(!is_newer("garbage", "1.0.0"));
    }
}
