use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::error::{AppError, AppResult};
use crate::paths::{empty_dir, login_script_dir};

/// POSIX single-quoting: everything inside is literal, and an embedded quote
/// is closed, escaped and reopened.
pub fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn script_file_name() -> &'static str {
    if cfg!(windows) {
        "login.cmd"
    } else if cfg!(target_os = "macos") {
        "login.command"
    } else {
        "login.sh"
    }
}

/// cmd's `set "K=V"` form takes the value literally, including `&` and `%`,
/// and the binary is invoked directly by cmd rather than through a second
/// shell, so the value is never expanded again.
pub fn windows_script(binary: &Path, config_dir: &Path) -> String {
    format!(
        "@echo off\r\nset \"CLAUDE_CONFIG_DIR={}\"\r\n\"{}\" /login\r\npause\r\n",
        config_dir.display(),
        binary.display()
    )
}

pub fn unix_script(binary: &Path, config_dir: &Path) -> String {
    format!(
        "#!/bin/bash\nexport CLAUDE_CONFIG_DIR={}\nexec {} /login\n",
        shell_single_quote(&config_dir.to_string_lossy()),
        shell_single_quote(&binary.to_string_lossy())
    )
}

/// Rust's automatic quoting only quotes when it sees a space, and cmd would
/// split an unquoted `&`, so the script path is always explicitly quoted. A
/// path containing a double quote cannot be quoted safely, so it is refused.
pub fn reject_quoted_path(path: &Path) -> AppResult<()> {
    if path.to_string_lossy().contains('"') {
        return Err(AppError::TerminalUnavailable(format!(
            "script path contains a double quote: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Writes the login helper into `<app_data_dir>/login/`, clearing the
/// directory first so nothing accumulates (spec 6.8).
pub fn write_login_script(
    app_data_dir: &Path,
    binary: &Path,
    config_dir: &Path,
) -> AppResult<PathBuf> {
    if !config_dir.is_dir() {
        return Err(AppError::NotFound(format!(
            "no such config directory: {}",
            config_dir.display()
        )));
    }

    let dir = login_script_dir(app_data_dir);
    empty_dir(&dir)?;

    let path = dir.join(script_file_name());
    reject_quoted_path(&path)?;

    let body = if cfg!(windows) {
        windows_script(binary, config_dir)
    } else {
        unix_script(binary, config_dir)
    };
    std::fs::write(&path, body)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| AppError::Io(format!("could not chmod {}: {e}", path.display())))?;
    }

    Ok(path)
}

#[cfg(windows)]
fn launch(script: &Path) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    let attempted = format!("cmd.exe /s /c start \"\" cmd.exe /k \"{}\"", script.display());
    std::process::Command::new("cmd.exe")
        .raw_arg(format!(
            "/s /c \"start \"\" cmd.exe /k \"{}\"\"",
            script.display()
        ))
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            warn!(error = %e, "could not open a login terminal");
            AppError::TerminalUnavailable(attempted)
        })
}

#[cfg(target_os = "macos")]
fn launch(script: &Path) -> AppResult<()> {
    let attempted = format!("open {}", script.display());
    std::process::Command::new("open")
        .arg(script)
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            warn!(error = %e, "could not open a login terminal");
            AppError::TerminalUnavailable(attempted)
        })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch(script: &Path) -> AppResult<()> {
    let candidates: [(&str, &str); 4] = [
        ("gnome-terminal", "--"),
        ("konsole", "-e"),
        ("xfce4-terminal", "-e"),
        ("xterm", "-e"),
    ];
    let mut attempted: Vec<String> = Vec::new();
    for (program, flag) in candidates {
        attempted.push(format!("{program} {flag} {}", script.display()));
        if std::process::Command::new(program)
            .arg(flag)
            .arg(script)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    warn!(attempted = ?attempted, "no terminal emulator could be launched");
    Err(AppError::TerminalUnavailable(attempted.join("; ")))
}

/// Opens a visible terminal running `<binary> /login` with `CLAUDE_CONFIG_DIR`
/// set. Paths are never interpolated into a shell command line: they go into
/// a script file the app writes and controls.
pub fn open_terminal_for_login(
    app_data_dir: &Path,
    binary: &Path,
    config_dir: &Path,
) -> AppResult<()> {
    let script = write_login_script(app_data_dir, binary, config_dir)?;
    info!(script = %script.display(), config_dir = %config_dir.display(), "opening login terminal");
    launch(&script)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn single_quoting_escapes_embedded_single_quotes() {
        assert_eq!(shell_single_quote("/home/josh"), "'/home/josh'");
        assert_eq!(
            shell_single_quote("/home/jo'sh/.claude"),
            r#"'/home/jo'\''sh/.claude'"#
        );
        assert_eq!(shell_single_quote(""), "''");
    }

    #[test]
    fn the_windows_script_sets_the_env_var_with_the_quoted_set_form() {
        let s = windows_script(
            &PathBuf::from(r"C:\Users\josh\.local\bin\claude.exe"),
            &PathBuf::from(r"C:\Users\josh\.claude3"),
        );
        assert!(s.starts_with("@echo off\r\n"));
        assert!(s.contains("set \"CLAUDE_CONFIG_DIR=C:\\Users\\josh\\.claude3\"\r\n"));
        assert!(s.contains("\"C:\\Users\\josh\\.local\\bin\\claude.exe\" /login\r\n"));
        assert!(s.trim_end().ends_with("pause"));
    }

    #[test]
    fn the_windows_script_takes_ampersands_and_percents_literally() {
        let s = windows_script(
            &PathBuf::from(r"C:\bin\claude.exe"),
            &PathBuf::from(r"C:\Users\a&b %USERNAME%\.claude"),
        );
        assert!(
            s.contains(r#"set "CLAUDE_CONFIG_DIR=C:\Users\a&b %USERNAME%\.claude""#),
            "the quoted set form takes the value literally: {s}"
        );
    }

    #[test]
    fn the_unix_script_single_quotes_both_paths() {
        let s = unix_script(
            &PathBuf::from("/home/josh/.local/bin/claude"),
            &PathBuf::from("/home/josh/.claude3"),
        );
        assert!(s.starts_with("#!/bin/bash\n"));
        assert!(s.contains("export CLAUDE_CONFIG_DIR='/home/josh/.claude3'\n"));
        assert!(s.contains("exec '/home/josh/.local/bin/claude' /login\n"));
    }

    #[test]
    fn the_unix_script_survives_a_path_with_a_space_and_a_dollar_sign() {
        let s = unix_script(
            &PathBuf::from("/home/josh/my bin/claude"),
            &PathBuf::from("/home/josh/$HOME dir/.claude"),
        );
        assert!(s.contains("export CLAUDE_CONFIG_DIR='/home/josh/$HOME dir/.claude'"));
        assert!(s.contains("exec '/home/josh/my bin/claude' /login"));
    }

    #[test]
    fn writing_the_script_empties_the_directory_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = crate::paths::login_script_dir(tmp.path());
        crate::paths::ensure_dir(&dir).expect("mkdir");
        std::fs::write(dir.join("stale.txt"), "old").expect("write stale");

        let binary = tmp.path().join("claude");
        let config = tmp.path().join(".claude3");
        std::fs::create_dir_all(&config).expect("cfg");
        std::fs::write(&binary, "x").expect("bin");

        let script = write_login_script(tmp.path(), &binary, &config).expect("write");
        assert!(script.is_file());
        assert!(!dir.join("stale.txt").exists(), "stale files are cleared");
        assert_eq!(
            script.file_name().and_then(|n| n.to_str()),
            Some(script_file_name())
        );
    }

    #[test]
    fn writing_the_script_overwrites_the_previous_one() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let binary = tmp.path().join("claude");
        let first = tmp.path().join(".claude");
        let second = tmp.path().join(".claude3");
        std::fs::create_dir_all(&first).expect("cfg1");
        std::fs::create_dir_all(&second).expect("cfg2");
        std::fs::write(&binary, "x").expect("bin");

        write_login_script(tmp.path(), &binary, &first).expect("first");
        let path = write_login_script(tmp.path(), &binary, &second).expect("second");
        let body = std::fs::read_to_string(&path).expect("read");
        assert!(body.contains(".claude3"));
        assert!(!body.contains("CLAUDE_CONFIG_DIR=") || body.matches("CLAUDE_CONFIG_DIR").count() == 1);
    }

    #[cfg(windows)]
    #[test]
    fn a_double_quote_in_the_script_path_is_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bad = tmp.path().join("we\"ird");
        // The rejection is about the resolved script path, so test the guard
        // directly rather than trying to create such a directory.
        let err = reject_quoted_path(&bad).expect_err("must reject");
        assert_eq!(err.code(), "terminal_unavailable");
    }

    #[test]
    fn a_clean_script_path_passes_the_quote_guard() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(reject_quoted_path(&tmp.path().join("login.cmd")).is_ok());
    }

    #[test]
    fn a_missing_config_dir_is_not_found() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let binary = tmp.path().join("claude");
        std::fs::write(&binary, "x").expect("bin");
        let err = write_login_script(tmp.path(), &binary, &tmp.path().join("nope"))
            .expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }
}
