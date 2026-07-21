use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Resolve the per-user data directory, creating it if needed.
/// `PET_TIMER_DATA_DIR` overrides the platform default (useful for tests).
pub fn data_dir() -> Result<PathBuf> {
    let dir = match std::env::var_os("PET_TIMER_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => directories::ProjectDirs::from("", "", "pet-timer")
            .context("could not determine a data directory for this platform")?
            .data_dir()
            .to_path_buf(),
    };
    fs::create_dir_all(&dir)
        .with_context(|| format!("could not create data dir {}", dir.display()))?;
    Ok(dir)
}

pub fn work_log_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("work_log.json"))
}

pub fn status_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("status.json"))
}

pub fn inbox_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("inbox.json"))
}

pub fn inbox_ack_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("inbox_ack.json"))
}

/// One-time migration: older versions wrote work_log.json into the CWD.
/// If the data dir has no log yet but the CWD does, copy it over.
pub fn migrate_legacy_log() -> Result<()> {
    let new = work_log_path()?;
    let old = Path::new("work_log.json");
    if !new.exists() && old.exists() {
        fs::copy(old, &new)
            .with_context(|| format!("could not migrate work_log.json to {}", new.display()))?;
    }
    Ok(())
}

/// Write via a sibling tmp file + rename so readers never see a torn file.
pub fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let mut tmp_name = path
        .file_name()
        .context("atomic_write needs a file path")?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    fs::write(&tmp, contents)
        .with_context(|| format!("could not write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("could not rename into {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_content() {
        let dir = std::env::temp_dir().join("pet-timer-test-atomic");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.json");
        atomic_write(&path, "one").unwrap();
        atomic_write(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert!(!dir.join("f.json.tmp").exists());
        fs::remove_dir_all(&dir).ok();
    }
}
