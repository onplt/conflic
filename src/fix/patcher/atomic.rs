use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn write_file_atomically(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let (temp_path, mut temp_file) = create_temp_sibling(path)?;

    let write_result = (|| -> std::io::Result<()> {
        temp_file.write_all(contents)?;
        temp_file.flush()?;
        temp_file.sync_all()?;
        drop(temp_file);

        std::fs::rename(&temp_path, path)?;
        sync_parent_directory(path)?;
        Ok(())
    })();

    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(())
}

fn create_temp_sibling(path: &Path) -> std::io::Result<(PathBuf, File)> {
    let directory = parent_dir(path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("conflic");

    for attempt in 0..32_u64 {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let candidate = directory.join(format!(
            ".{}.conflic.tmp.{}.{}.{}",
            file_name,
            std::process::id(),
            counter,
            timestamp + attempt as u128
        ));

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("Failed to allocate a temporary file for {}", path.display()),
    ))
}

fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent_dir(path))?.sync_all()?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}
