//! Short-lived GitHub results. Cache keys include the working directory, repository
//! config, invocation and active GitHub CLI settings so contexts stay separate.
use super::{query_with_timeout, record};
use crate::util::{config_home, query_result_scoped, QueryStatus};
use sha1_smol::Sha1;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

const TTL: Duration = Duration::from_secs(20);
const LIMIT: usize = 400_000;

pub(super) fn github(scope: &[String], args: &[&str]) -> String {
    if !super::permits_query("gh", scope, args) {
        return String::new();
    }
    let cache = cache_file(scope, args);
    if let Some(data) = cache.as_deref().and_then(fresh) {
        record(|| {
            serde_json::json!({
                "source": "gh-cache", "operation": format!("{} list", args[0]),
            "slot": if args[0] == "pr" { "pull request number" } else { "issue number" },
                "scope": if scope.is_empty() { "current" } else { "explicit" }, "selectors": scope,
                "status": "hit"
            })
        });
        return data;
    }
    let (data, status) = query_with_timeout("gh", scope, args, Duration::from_millis(850));
    if status == QueryStatus::Success && serde_json::from_str::<Vec<serde_json::Value>>(&data).is_ok() {
        if let Some(path) = cache {
            let _ = write_private(&path, &data);
            if let Some(dir) = path.parent() {
                prune_expired(dir);
            }
        }
    }
    data
}

fn cache_file(scope: &[String], args: &[&str]) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut key = Sha1::new();
    let mut add = |part: &[u8]| {
        key.update(&part.len().to_le_bytes());
        key.update(part);
    };
    add(cwd.as_os_str().as_encoded_bytes());
    for arg in scope.iter().map(String::as_str).chain(args.iter().copied()) {
        add(arg.as_bytes());
    }
    for env in [
        "GH_HOST",
        "GH_REPO",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "PATH",
    ] {
        add(super::effective_env("gh", env).unwrap_or_default().as_encoded_bytes());
    }
    let cfg = super::effective_env("gh", "GH_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_home().join("gh"));
    add(cfg.as_os_str().as_encoded_bytes());
    let hosts = cfg.join("hosts.yml");
    if hosts.is_file() {
        // GitHub CLI stores its active user in hosts.yml. A file change
        // invalidates these results without reading credential material.
        let meta = hosts.metadata().ok()?;
        add(&meta.mtime().to_le_bytes());
        add(&meta.mtime_nsec().to_le_bytes());
        add(&meta.len().to_le_bytes());
    }
    if scope.is_empty() {
        let overrides = super::ACTIVE_SCOPE.with_borrow(|scope| scope.for_tool("git"));
        let (settings, _) = query_result_scoped(
            &[
                "git".into(),
                "config".into(),
                "--get-regexp".into(),
                r"^remote\..*\.(url|gh-resolved)$".into(),
            ],
            Duration::from_millis(100),
            &overrides,
        );
        if settings.is_empty() {
            return None;
        }
        add(settings.as_bytes());
    }
    let dir = crate::util::cache_dir("data").join("gh");
    let _ = std::fs::DirBuilder::new().mode(0o700).create(&dir);
    let meta = std::fs::symlink_metadata(&dir).ok()?;
    if !meta.is_dir() || meta.mode() & 0o077 != 0 || meta.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    Some(dir.join(key.digest().to_string()))
}

fn fresh(path: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file()
        || meta.mode() & 0o077 != 0
        || meta.len() > LIMIT as u64
        || meta.modified().ok()?.elapsed().ok()? >= TTL
    {
        return None;
    }
    let mut data = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(LIMIT as u64)
        .read_to_string(&mut data)
        .ok()?;
    Some(data)
}

fn write_private(path: &Path, data: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let result = file
        .write_all(data.as_bytes())
        .and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn prune_expired(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.as_encoded_bytes();
        if name.len() != 40 || !name.iter().all(u8::is_ascii_hexdigit) {
            continue;
        }
        let path = entry.path();
        let age = std::fs::symlink_metadata(&path)
            .ok()
            .filter(|meta| meta.is_file())
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.elapsed().ok());
        if age.is_some_and(|age| age >= TTL) {
            let _ = std::fs::remove_file(path);
        }
    }
}
