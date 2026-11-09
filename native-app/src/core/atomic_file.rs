use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Write beside the destination, then replace it only after the complete file is durable.
pub fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination path has no parent directory",
        )
    })?;

    for _ in 0..16 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = parent.join(format!(
            ".carambola-save-{}-{nanos}-{sequence}.tmp",
            std::process::id()
        ));

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
                    drop(file);
                    let _ = fs::remove_file(&temporary);
                    return Err(error);
                }
                drop(file);

                if let Err(error) = fs::rename(&temporary, path) {
                    let _ = fs::remove_file(&temporary);
                    return Err(error);
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary file",
    ))
}

#[cfg(test)]
mod tests {
    use super::write;
    use std::{fs, path::PathBuf, time::SystemTime};

    fn scratch_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "carambola-atomic-file-{}-{nonce}",
                std::process::id()
            ))
            .join("value.json")
    }

    #[test]
    fn writes_new_file_and_replaces_existing_file() {
        let path = scratch_path();
        let parent = path.parent().unwrap().to_path_buf();
        fs::create_dir_all(&parent).unwrap();

        write(&path, b"first").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");

        write(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");

        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        fs::remove_file(&path).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn missing_parent_does_not_create_a_destination() {
        let path = scratch_path();
        assert!(write(&path, b"content").is_err());
        assert!(!path.exists());
    }
}
