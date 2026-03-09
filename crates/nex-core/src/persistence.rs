use crate::CodexResult;
use serde::{Serialize, de::DeserializeOwned};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> CodexResult<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file_mut().sync_all()?;

    let backup = backup_path(path);
    remove_file_if_exists(&backup)?;

    let had_primary = path.exists();
    if had_primary {
        std::fs::rename(path, &backup)?;
    }

    match temp.persist(path) {
        Ok(_) => {
            let _ = remove_file_if_exists(&backup);
            sync_dir(parent);
            Ok(())
        }
        Err(err) => {
            if had_primary && !path.exists() && backup.exists() {
                let _ = std::fs::rename(&backup, path);
            }
            Err(err.error.into())
        }
    }
}

pub fn atomic_write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> CodexResult<()> {
    let content = serde_json::to_vec_pretty(value)?;
    atomic_write_bytes(path, &content)
}

pub fn load_json_with_backup<T: DeserializeOwned>(path: &Path) -> CodexResult<Option<T>> {
    let backup = backup_path(path);
    match try_load_json(path) {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => try_load_json(&backup),
        Err(primary_err) => match try_load_json(&backup) {
            Ok(Some(value)) => Ok(Some(value)),
            _ => Err(primary_err),
        },
    }
}

pub fn load_bytes_with_backup(path: &Path) -> CodexResult<Option<Vec<u8>>> {
    let backup = backup_path(path);
    match try_load_bytes(path) {
        Ok(Some(bytes)) => Ok(Some(bytes)),
        Ok(None) => try_load_bytes(&backup),
        Err(primary_err) => match try_load_bytes(&backup) {
            Ok(Some(bytes)) => Ok(Some(bytes)),
            _ => Err(primary_err),
        },
    }
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".bak");
    PathBuf::from(backup)
}

fn try_load_json<T: DeserializeOwned>(path: &Path) -> CodexResult<Option<T>> {
    let Some(content) = try_read_to_string(path)? else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str(&content)?))
}

fn try_load_bytes(path: &Path) -> CodexResult<Option<Vec<u8>>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read(path)?))
}

fn try_read_to_string(path: &Path) -> CodexResult<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read_to_string(path)?))
}

fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(not(windows))]
fn sync_dir(path: &Path) {
    if let Ok(dir) = std::fs::File::open(path) {
        let _ = dir.sync_all();
    }
}

#[cfg(windows)]
fn sync_dir(_path: &Path) {}
