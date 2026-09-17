use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// The user's home directory.
pub fn home_dir() -> AppResult<PathBuf> {
    dirs::home_dir().ok_or_else(|| AppError::NotFound("home directory not found".into()))
}

pub fn db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("usage.sqlite")
}

/// The working directory every poll child is spawned in (spec 6.3).
pub fn poll_cwd(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("poll-cwd")
}

/// Where the login helper script is written (spec 6.8).
pub fn login_script_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("login")
}

pub fn ensure_dir(dir: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dir).map_err(|e| {
        AppError::Io(format!("could not create {}: {e}", dir.display()))
    })
}

/// Remove everything inside `dir`, creating `dir` if it does not exist.
pub fn empty_dir(dir: &Path) -> AppResult<()> {
    if !dir.exists() {
        return ensure_dir(dir);
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", dir.display())))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| AppError::Io(format!("could not read {}: {e}", dir.display())))?;
        let path = entry.path();
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        result.map_err(|e| AppError::Io(format!("could not remove {}: {e}", path.display())))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use super::{db_path, empty_dir, ensure_dir, login_script_dir, poll_cwd};

    #[test]
    fn derived_paths_hang_off_the_app_data_dir() {
        let base = Path::new("/data/cut");
        assert_eq!(db_path(base), PathBuf::from("/data/cut/usage.sqlite"));
        assert_eq!(poll_cwd(base), PathBuf::from("/data/cut/poll-cwd"));
        assert_eq!(login_script_dir(base), PathBuf::from("/data/cut/login"));
    }

    #[test]
    fn ensure_dir_creates_nested_directories_and_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("a").join("b").join("c");
        ensure_dir(&target).expect("first create");
        assert!(target.is_dir());
        ensure_dir(&target).expect("second create is a no-op");
        assert!(target.is_dir());
    }

    #[test]
    fn empty_dir_removes_contents_but_keeps_the_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("login");
        ensure_dir(&target).expect("create");
        std::fs::write(target.join("login.cmd"), "old").expect("write file");
        std::fs::create_dir(target.join("sub")).expect("create sub");
        std::fs::write(target.join("sub").join("x"), "old").expect("write nested");

        empty_dir(&target).expect("empty");

        assert!(target.is_dir());
        let left = std::fs::read_dir(&target)
            .expect("read_dir")
            .count();
        assert_eq!(left, 0);
    }

    #[test]
    fn empty_dir_creates_the_directory_when_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("nope");
        empty_dir(&target).expect("empty");
        assert!(target.is_dir());
    }
}
