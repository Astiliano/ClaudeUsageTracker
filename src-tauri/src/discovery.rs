use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// D3: the exact message shown when a Windows `.cmd` / `.bat` shim is found.
pub const CMD_SHIM_MESSAGE: &str =
    "npm shim not supported; install the native build (`claude install`) or point Settings at `claude.exe`";

const CONFIG_DIR_MARKERS: [&str; 3] = [".credentials.json", ".claude.json", "settings.json"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinarySource {
    Override,
    LocalBin,
    Path,
}

impl BinarySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinarySource::Override => "override",
            BinarySource::LocalBin => "local_bin",
            BinarySource::Path => "path",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub source: BinarySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub config_dir: PathBuf,
    pub label: String,
}

/// D3 acceptance, expressed purely so it can be tested on any host:
/// a regular file, and on Windows the extension must be exactly `exe`
/// (case-insensitively); elsewhere an executable mode bit must be set.
pub fn accept_candidate(
    path: &Path,
    is_windows: bool,
    is_file: bool,
    unix_mode: Option<u32>,
) -> bool {
    if !is_file {
        return false;
    }
    if is_windows {
        return matches!(
            path.extension().and_then(|e| e.to_str()),
            Some(ext) if ext.eq_ignore_ascii_case("exe")
        );
    }
    matches!(unix_mode, Some(m) if m & 0o111 != 0)
}

/// Apply `accept_candidate` to a real path, and log the D3 skip for a shim.
fn is_acceptable_binary(path: &Path) -> bool {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode())
    };
    #[cfg(not(unix))]
    let mode: Option<u32> = None;

    let accepted = accept_candidate(path, cfg!(windows), meta.is_file(), mode);
    if !accepted && meta.is_file() && cfg!(windows) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat") {
                warn!(path = %path.display(), "{}", CMD_SHIM_MESSAGE);
            }
        }
    }
    accepted
}

fn binary_file_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["claude.exe", "claude.cmd", "claude.bat", "claude"]
    } else {
        &["claude"]
    }
}

/// Settings override, then `<home>/.local/bin/claude[.exe]`, then the first
/// acceptable hit walking `path_dirs` in order (spec 6.1).
pub fn find_claude_binary_in(
    override_path: Option<&str>,
    home: &Path,
    path_dirs: &[PathBuf],
) -> Option<Found> {
    if let Some(raw) = override_path.map(str::trim) {
        if !raw.is_empty() {
            let p = PathBuf::from(raw);
            if is_acceptable_binary(&p) {
                info!(path = %p.display(), source = "override", "claude binary selected");
                return Some(Found {
                    path: p,
                    source: BinarySource::Override,
                });
            }
        }
    }

    let local_bin = home.join(".local").join("bin");
    for name in binary_file_names() {
        let p = local_bin.join(name);
        if is_acceptable_binary(&p) {
            info!(path = %p.display(), source = "local_bin", "claude binary selected");
            return Some(Found {
                path: p,
                source: BinarySource::LocalBin,
            });
        }
    }

    for dir in path_dirs {
        for name in binary_file_names() {
            let p = dir.join(name);
            if is_acceptable_binary(&p) {
                info!(path = %p.display(), source = "path", "claude binary selected");
                return Some(Found {
                    path: p,
                    source: BinarySource::Path,
                });
            }
        }
    }

    None
}

/// Production wrapper: real home directory and the real `PATH`.
pub fn find_claude_binary(override_path: Option<&str>) -> Option<Found> {
    let home = crate::paths::home_dir().ok()?;
    let path_dirs: Vec<PathBuf> = match std::env::var_os("PATH") {
        Some(p) => std::env::split_paths(&p).collect(),
        None => Vec::new(),
    };
    find_claude_binary_in(override_path, &home, &path_dirs)
}

/// Every `<home>/.claude*` directory carrying at least one config marker,
/// canonicalised with `dunce` so Windows paths have no verbatim prefix.
/// Sorted case-insensitively by label; default-first ordering (D17) is
/// applied by the store when it lists accounts.
pub fn enumerate_profiles(home: &Path) -> Vec<Candidate> {
    let entries = match std::fs::read_dir(home) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut out: Vec<Candidate> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.starts_with(".claude") {
            continue;
        }
        if !CONFIG_DIR_MARKERS.iter().any(|m| path.join(m).is_file()) {
            continue;
        }
        let config_dir = dunce::canonicalize(&path).unwrap_or(path);
        let label = name.trim_start_matches('.').to_string();
        out.push(Candidate { config_dir, label });
    }

    out.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then_with(|| a.label.cmp(&b.label))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn windows_accepts_only_exe() {
        assert!(accept_candidate(
            Path::new("C:/Users/josh/.local/bin/claude.exe"),
            true,
            true,
            None
        ));
        assert!(accept_candidate(
            Path::new("C:/Users/josh/.local/bin/CLAUDE.EXE"),
            true,
            true,
            None
        ));
    }

    #[test]
    fn windows_rejects_cmd_and_bat_shims() {
        assert!(!accept_candidate(
            Path::new("C:/Users/josh/AppData/Roaming/npm/claude.cmd"),
            true,
            true,
            None
        ));
        assert!(!accept_candidate(
            Path::new("C:/Users/josh/AppData/Roaming/npm/claude.bat"),
            true,
            true,
            None
        ));
        assert!(!accept_candidate(
            Path::new("C:/Users/josh/AppData/Roaming/npm/claude"),
            true,
            true,
            None
        ));
    }

    #[test]
    fn windows_rejects_a_directory() {
        assert!(!accept_candidate(
            Path::new("C:/Users/josh/.local/bin/claude.exe"),
            true,
            false,
            None
        ));
    }

    #[test]
    fn unix_requires_an_executable_mode_bit() {
        assert!(accept_candidate(
            Path::new("/home/josh/.local/bin/claude"),
            false,
            true,
            Some(0o755)
        ));
        assert!(!accept_candidate(
            Path::new("/home/josh/.local/bin/claude"),
            false,
            true,
            Some(0o644)
        ));
        assert!(!accept_candidate(
            Path::new("/home/josh/.local/bin/claude"),
            false,
            false,
            Some(0o755)
        ));
    }

    #[test]
    fn the_shim_message_is_the_spec_wording() {
        assert_eq!(
            CMD_SHIM_MESSAGE,
            "npm shim not supported; install the native build (`claude install`) or point Settings at `claude.exe`"
        );
    }

    #[test]
    fn binary_source_wire_forms_are_snake_case() {
        assert_eq!(BinarySource::Override.as_str(), "override");
        assert_eq!(BinarySource::LocalBin.as_str(), "local_bin");
        assert_eq!(BinarySource::Path.as_str(), "path");
    }

    /// Create a file that `accept_candidate` will accept on this platform.
    fn write_executable(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, b"#!/bin/sh\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
    }

    fn binary_name() -> &'static str {
        if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        }
    }

    #[test]
    fn override_wins_over_local_bin_and_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let over = tmp.path().join("custom").join(binary_name());
        let local = home.join(".local").join("bin").join(binary_name());
        let on_path_dir = tmp.path().join("pathdir");
        let on_path = on_path_dir.join(binary_name());
        write_executable(&over);
        write_executable(&local);
        write_executable(&on_path);

        let found = find_claude_binary_in(
            Some(&over.to_string_lossy()),
            &home,
            std::slice::from_ref(&on_path_dir),
        )
        .expect("found");
        assert_eq!(found.source, BinarySource::Override);
        assert_eq!(found.path, over);
    }

    #[test]
    fn local_bin_wins_over_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let local = home.join(".local").join("bin").join(binary_name());
        let on_path_dir = tmp.path().join("pathdir");
        write_executable(&local);
        write_executable(&on_path_dir.join(binary_name()));

        let found = find_claude_binary_in(None, &home, &[on_path_dir]).expect("found");
        assert_eq!(found.source, BinarySource::LocalBin);
        assert_eq!(found.path, local);
    }

    #[test]
    fn falls_back_to_the_first_path_hit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let first = tmp.path().join("p1");
        let second = tmp.path().join("p2");
        write_executable(&first.join(binary_name()));
        write_executable(&second.join(binary_name()));

        let found = find_claude_binary_in(None, &home, &[first.clone(), second]).expect("found");
        assert_eq!(found.source, BinarySource::Path);
        assert_eq!(found.path, first.join(binary_name()));
    }

    #[test]
    fn a_blank_override_is_ignored() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let local = home.join(".local").join("bin").join(binary_name());
        write_executable(&local);

        let found = find_claude_binary_in(Some("  "), &home, &[]).expect("found");
        assert_eq!(found.source, BinarySource::LocalBin);
    }

    #[test]
    fn nothing_found_returns_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        assert!(find_claude_binary_in(None, &home, &[]).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn a_cmd_shim_on_path_is_skipped_entirely() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let shim_dir = tmp.path().join("npm");
        write_executable(&shim_dir.join("claude.cmd"));
        assert!(find_claude_binary_in(None, &home, &[shim_dir]).is_none());
    }

    /// Build the spec 2.3 home layout in a temp dir.
    fn profile_home() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();

        let dot_claude = home.join(".claude");
        fs::create_dir_all(dot_claude.join("projects")).expect("mk .claude");
        fs::write(dot_claude.join(".credentials.json"), "{}").expect("w");
        fs::write(dot_claude.join("settings.json"), "{}").expect("w");

        for name in [".claude2", ".claude3"] {
            let d = home.join(name);
            fs::create_dir_all(&d).expect("mk");
            fs::write(d.join(".credentials.json"), "{}").expect("w");
            fs::write(d.join(".claude.json"), "{}").expect("w");
            fs::write(d.join("settings.json"), "{}").expect("w");
        }

        for name in [".claude-free", ".claude-kilofree"] {
            let d = home.join(name);
            fs::create_dir_all(d.join("projects")).expect("mk");
            fs::write(d.join(".claude.json"), "{}").expect("w");
        }

        let flow = home.join(".claude-flow");
        fs::create_dir_all(&flow).expect("mk");
        fs::write(flow.join("update-state.json"), "{}").expect("w");

        // A non-matching directory and a matching-looking plain file.
        fs::create_dir_all(home.join(".config")).expect("mk");
        fs::write(home.join(".claude.json"), "{}").expect("w");

        tmp
    }

    #[test]
    fn enumerate_profiles_finds_exactly_the_config_dirs() {
        let tmp = profile_home();
        let found = enumerate_profiles(tmp.path());
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["claude", "claude-free", "claude-kilofree", "claude2", "claude3"]
        );
    }

    #[test]
    fn enumerate_profiles_excludes_the_update_state_only_dir() {
        let tmp = profile_home();
        let found = enumerate_profiles(tmp.path());
        assert!(
            found.iter().all(|c| c.label != "claude-flow"),
            "a dir with only update-state.json is not a config dir"
        );
    }

    #[test]
    fn enumerate_profiles_excludes_plain_files() {
        let tmp = profile_home();
        let found = enumerate_profiles(tmp.path());
        assert!(found.iter().all(|c| c.config_dir.is_dir()));
    }

    #[test]
    fn enumerate_profiles_canonicalises_without_a_verbatim_prefix() {
        let tmp = profile_home();
        let found = enumerate_profiles(tmp.path());
        for c in &found {
            let s = c.config_dir.to_string_lossy().to_string();
            assert!(!s.starts_with(r"\\?\"), "unexpected verbatim prefix: {s}");
        }
    }

    #[test]
    fn enumerate_profiles_is_sorted_case_insensitively_by_label() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for name in [".claudeZ", ".claudea", ".claudeB"] {
            let d = tmp.path().join(name);
            fs::create_dir_all(&d).expect("mk");
            fs::write(d.join("settings.json"), "{}").expect("w");
        }
        let found = enumerate_profiles(tmp.path());
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["claudea", "claudeB", "claudeZ"]);
    }

    #[test]
    fn enumerate_profiles_on_a_missing_home_returns_empty() {
        let missing = PathBuf::from("/definitely/not/a/home/dir/here");
        assert!(enumerate_profiles(&missing).is_empty());
    }
}
