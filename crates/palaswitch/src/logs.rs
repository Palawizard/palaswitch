//! Log file of the app, kept under a fixed size.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Size at which the log file is moved aside. With the one older file kept,
/// logs never take more than twice this on disk.
pub const MAX_LOG_LEN: u64 = 1024 * 1024;

const FILE_NAME: &str = "palaswitch.log";
const PREVIOUS_FILE_NAME: &str = "palaswitch.1.log";

/// `%LOCALAPPDATA%\palaswitch\logs`, when Windows gives that folder.
pub fn default_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(Path::new(&local).join("palaswitch").join("logs"))
}

/// Sends `tracing` events to a rotating file in `dir`. Without a usable
/// folder the app runs without logs rather than not at all.
pub fn init(dir: Option<&Path>) {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_target(false);
    let file = dir.map(|dir| RotatingFile::open(dir, MAX_LOG_LEN));
    match file {
        Some(Ok(file)) => subscriber.with_writer(Mutex::new(file)).init(),
        _ => subscriber.with_writer(io::sink).init(),
    }
}

/// A log file that is renamed to `palaswitch.1.log` once a write would make
/// it longer than `max_len`, replacing the older file.
#[derive(Debug)]
pub struct RotatingFile {
    path: PathBuf,
    previous: PathBuf,
    max_len: u64,
    file: File,
    len: u64,
}

impl RotatingFile {
    pub fn open(dir: &Path, max_len: u64) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(FILE_NAME);
        let file = append(&path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            path,
            previous: dir.join(PREVIOUS_FILE_NAME),
            max_len,
            file,
            len,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        // Windows refuses to rename a file over an existing one.
        match fs::remove_file(&self.previous) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        fs::rename(&self.path, &self.previous)?;
        self.file = append(&self.path)?;
        self.len = 0;
        Ok(())
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.len > 0 && self.len + buf.len() as u64 > self.max_len && self.rotate().is_err() {
            // Another program holds a log file open: start this one over
            // rather than grow past the limit or stop logging.
            self.file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&self.path)?;
            self.len = 0;
        }
        let written = self.file.write(buf)?;
        self.len += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp folder, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("palaswitch-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn line(n: usize) -> Vec<u8> {
        format!("line {n:04}\n").into_bytes()
    }

    #[test]
    fn moves_the_file_aside_once_it_is_full() {
        let dir = TempDir::new("rotate");
        let mut file = RotatingFile::open(&dir.0, 30).unwrap();

        for n in 0..4 {
            file.write_all(&line(n)).unwrap();
        }

        let current = fs::read_to_string(dir.0.join(FILE_NAME)).unwrap();
        let previous = fs::read_to_string(dir.0.join(PREVIOUS_FILE_NAME)).unwrap();
        assert_eq!(previous, "line 0000\nline 0001\nline 0002\n");
        assert_eq!(current, "line 0003\n");
    }

    #[test]
    fn keeps_only_one_older_file() {
        let dir = TempDir::new("bounded");
        let mut file = RotatingFile::open(&dir.0, 30).unwrap();

        for n in 0..100 {
            file.write_all(&line(n)).unwrap();
        }

        let mut names: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, [PREVIOUS_FILE_NAME, FILE_NAME]);
        let total: u64 = [FILE_NAME, PREVIOUS_FILE_NAME]
            .iter()
            .map(|name| fs::metadata(dir.0.join(name)).unwrap().len())
            .sum();
        assert!(total <= 60, "{total} bytes on disk");
        let previous = fs::read_to_string(dir.0.join(PREVIOUS_FILE_NAME)).unwrap();
        assert_eq!(previous, "line 0096\nline 0097\nline 0098\n");
    }

    #[test]
    fn continues_an_existing_file_and_counts_its_size() {
        let dir = TempDir::new("reopen");
        fs::create_dir_all(&dir.0).unwrap();
        fs::write(dir.0.join(FILE_NAME), "line 0000\nline 0001\n").unwrap();
        let mut file = RotatingFile::open(&dir.0, 30).unwrap();

        file.write_all(&line(2)).unwrap();
        file.write_all(&line(3)).unwrap();

        let previous = fs::read_to_string(dir.0.join(PREVIOUS_FILE_NAME)).unwrap();
        assert_eq!(previous, "line 0000\nline 0001\nline 0002\n");
    }

    #[cfg(windows)]
    #[test]
    fn starts_over_when_another_program_blocks_the_rotation() {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ_WRITE: u32 = 0x1 | 0x2;
        let dir = TempDir::new("blocked");
        let mut file = RotatingFile::open(&dir.0, 30).unwrap();
        for n in 0..3 {
            file.write_all(&line(n)).unwrap();
        }
        // Without FILE_SHARE_DELETE, the log file cannot be renamed.
        let _blocker = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ_WRITE)
            .open(dir.0.join(FILE_NAME))
            .unwrap();

        file.write_all(&line(3)).unwrap();

        let current = fs::read_to_string(dir.0.join(FILE_NAME)).unwrap();
        assert_eq!(
            current,
            "line 0003
"
        );
    }

    #[test]
    fn writes_an_event_longer_than_the_limit_in_one_piece() {
        let dir = TempDir::new("long");
        let mut file = RotatingFile::open(&dir.0, 4).unwrap();

        file.write_all(&line(0)).unwrap();

        let current = fs::read_to_string(dir.0.join(FILE_NAME)).unwrap();
        assert_eq!(current, "line 0000\n");
        assert!(!dir.0.join(PREVIOUS_FILE_NAME).exists());
    }
}
