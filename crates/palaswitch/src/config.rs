//! Settings file: `%APPDATA%\palaswitch\settings.toml`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::shortcut::Shortcut;

const FILE_NAME: &str = "settings.toml";
/// Highest profile sector the settings accept; the settings window keeps
/// the sector in the low bits of its control ids.
pub const MAX_SECTOR: u16 = 0x0FFF;
/// Longest display name of a profile, in characters.
pub const MAX_NAME_LEN: usize = 32;

/// `%APPDATA%\palaswitch\settings.toml`, when Windows gives that folder.
pub fn default_path() -> Option<PathBuf> {
    let roaming = std::env::var_os("APPDATA")?;
    Some(Path::new(&roaming).join("palaswitch").join(FILE_NAME))
}

/// What the user chose for one profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileSettings {
    /// Shown instead of "Profile n".
    pub name: Option<String>,
    pub shortcut: Option<Shortcut>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub start_with_windows: bool,
    /// By profile sector, the value the mouse uses for the profile.
    pub profiles: BTreeMap<u16, ProfileSettings>,
}

/// The file as written, before it is checked.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    start_with_windows: bool,
    #[serde(default, rename = "profile")]
    profiles: Vec<FileProfile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileProfile {
    /// Profile sector: 1 for the first profile.
    sector: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcut: Option<String>,
}

/// The settings could not be used as they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reset {
    /// The file is not valid; it was moved to `kept_at` when that worked.
    Invalid {
        reason: String,
        kept_at: Option<PathBuf>,
    },
    /// The file could not be read, maybe only for a moment (another
    /// program had it open): it is left in place.
    Unreadable { reason: String },
}

impl Reset {
    /// Whether the file the app would write over is still the user's: then
    /// nothing is saved until the next start.
    pub fn file_left_in_place(&self) -> bool {
        !matches!(
            self,
            Self::Invalid {
                kept_at: Some(_),
                ..
            }
        )
    }
}

/// Reads the settings. A missing file gives the defaults; an invalid one is
/// moved to `settings.toml.bak` and also gives the defaults.
pub fn load(path: &Path) -> (Settings, Option<Reset>) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (Settings::default(), None);
        }
        Err(error) => {
            tracing::warn!(%error, "settings file unreadable, defaults used");
            let reason = error.to_string();
            return (Settings::default(), Some(Reset::Unreadable { reason }));
        }
    };
    // A file saved in another encoding is not valid, and stays so.
    let parsed = String::from_utf8(bytes)
        .map_err(|_| "not UTF-8 text".to_owned())
        .and_then(|text| parse(&text));
    match parsed {
        Ok(settings) => (settings, None),
        Err(reason) => {
            let backup = backup_path(path);
            let kept_at = fs::rename(path, &backup).ok().map(|()| backup);
            tracing::warn!(reason, ?kept_at, "settings reset to defaults");
            (
                Settings::default(),
                Some(Reset::Invalid { reason, kept_at }),
            )
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".bak");
    path.with_file_name(name)
}

pub fn parse(text: &str) -> Result<Settings, String> {
    let file: File = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut settings = Settings {
        start_with_windows: file.start_with_windows,
        profiles: BTreeMap::new(),
    };
    for entry in file.profiles {
        if entry.sector == 0 || entry.sector > MAX_SECTOR {
            return Err(format!("profile sector {} does not exist", entry.sector));
        }
        if let Some(name) = &entry.name
            && let Err(problem) = check_name(name)
        {
            return Err(format!("profile {}: {problem}", entry.sector));
        }
        let shortcut = match &entry.shortcut {
            Some(text) => {
                let shortcut: Shortcut = text.parse().map_err(|error| format!("{error}"))?;
                shortcut
                    .check()
                    .map_err(|refusal| format!("{text}: {refusal:?}"))?;
                Some(shortcut)
            }
            None => None,
        };
        let profile = ProfileSettings {
            name: entry.name,
            shortcut,
        };
        if settings.profiles.insert(entry.sector, profile).is_some() {
            return Err(format!("profile {} is listed twice", entry.sector));
        }
    }
    let mut shortcuts: Vec<Shortcut> = settings
        .profiles
        .values()
        .filter_map(|profile| profile.shortcut)
        .collect();
    shortcuts.sort_by_key(|shortcut| shortcut.to_string());
    if let Some(pair) = shortcuts.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(format!("{} is used by two profiles", pair[0]));
    }
    Ok(settings)
}

/// Why a display name cannot be used.
pub fn check_name(name: &str) -> Result<(), &'static str> {
    if name.chars().count() > MAX_NAME_LEN {
        Err("name too long")
    } else if name.chars().any(char::is_control) {
        Err("name with control characters")
    } else {
        Ok(())
    }
}

pub fn to_text(settings: &Settings) -> String {
    let file = File {
        start_with_windows: settings.start_with_windows,
        profiles: settings
            .profiles
            .iter()
            .filter(|(_, profile)| profile.name.is_some() || profile.shortcut.is_some())
            .map(|(&sector, profile)| FileProfile {
                sector,
                name: profile.name.clone(),
                shortcut: profile.shortcut.map(|shortcut| shortcut.to_string()),
            })
            .collect(),
    };
    toml::to_string(&file).unwrap_or_default()
}

/// Writes the settings to a temporary file, then moves it over the old
/// one, so that a crash never leaves half a file.
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut file = fs::File::create(&temporary)?;
    file.write_all(to_text(settings).as_bytes())?;
    // On disk before the rename, so that a power cut cannot leave an
    // empty file in place of the settings.
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp folder, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("palaswitch-config-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self) -> PathBuf {
            self.0.join(FILE_NAME)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const VALID: &str = r#"start_with_windows = true

[[profile]]
sector = 1
name = "Jeu"
shortcut = "Ctrl+Alt+F1"

[[profile]]
sector = 2
shortcut = "Ctrl+Alt+F2"
"#;

    #[test]
    fn valid_file_is_read() {
        let settings = parse(VALID).expect("valid");

        assert!(settings.start_with_windows);
        assert_eq!(settings.profiles[&1].name.as_deref(), Some("Jeu"));
        assert_eq!(
            settings.profiles[&2].shortcut.map(|s| s.to_string()),
            Some("Ctrl+Alt+F2".to_owned())
        );
    }

    #[test]
    fn settings_written_read_back_the_same() {
        let settings = parse(VALID).expect("valid");

        assert_eq!(parse(&to_text(&settings)), Ok(settings));
    }

    #[test]
    fn invalid_files_are_refused() {
        let cases = [
            "start_with_windows = maybe",
            "colour = \"blue\"",
            "[[profile]]\nsector = 0",
            "[[profile]]\nsector = 4096",
            "[[profile]]\nsector = 1\n[[profile]]\nsector = 1",
            "[[profile]]\nsector = 1\nshortcut = \"Ctrl+Nope\"",
            "[[profile]]\nsector = 1\nshortcut = \"A\"",
            "[[profile]]\nsector = 1\nname = \"a name far too long for the settings window\"",
            "[[profile]]\nsector = 1\nshortcut = \"Ctrl+Alt+F1\"\n[[profile]]\nsector = 2\nshortcut = \"ctrl+alt+f1\"",
        ];

        for case in cases {
            assert!(parse(case).is_err(), "accepted: {case}");
        }
    }

    #[test]
    fn missing_file_gives_defaults_without_complaint() {
        let dir = TempDir::new("missing");

        assert_eq!(load(&dir.file()), (Settings::default(), None));
    }

    #[test]
    fn invalid_file_is_kept_aside_and_defaults_used() {
        let dir = TempDir::new("invalid");
        fs::write(dir.file(), "start_with_windows = maybe").unwrap();

        let (settings, reset) = load(&dir.file());

        let reset = reset.expect("reset");
        assert_eq!(settings, Settings::default());
        assert!(!reset.file_left_in_place());
        let Reset::Invalid { kept_at, .. } = reset else {
            panic!("not invalid: {reset:?}");
        };
        assert_eq!(kept_at, Some(dir.0.join("settings.toml.bak")));
        assert!(!dir.file().exists());
        assert_eq!(
            fs::read_to_string(dir.0.join("settings.toml.bak")).unwrap(),
            "start_with_windows = maybe"
        );
    }

    #[test]
    fn file_in_another_encoding_is_kept_aside() {
        let dir = TempDir::new("encoding");
        // "start_with_windows = true # é" in Windows-1252.
        let mut bytes = b"start_with_windows = true # ".to_vec();
        bytes.push(0xE9);
        fs::write(dir.file(), &bytes).unwrap();

        let (_, reset) = load(&dir.file());

        assert!(matches!(
            reset,
            Some(Reset::Invalid {
                kept_at: Some(_),
                ..
            })
        ));
        assert_eq!(fs::read(dir.0.join("settings.toml.bak")).unwrap(), bytes);
    }

    #[test]
    fn unreadable_file_is_left_in_place() {
        let dir = TempDir::new("unreadable");
        // A folder where the file should be cannot be read as a file.
        fs::create_dir_all(dir.file()).unwrap();

        let (settings, reset) = load(&dir.file());

        let reset = reset.expect("reset");
        assert_eq!(settings, Settings::default());
        assert!(matches!(reset, Reset::Unreadable { .. }));
        assert!(reset.file_left_in_place());
        assert!(dir.file().is_dir());
        assert!(!dir.0.join("settings.toml.bak").exists());
    }

    #[test]
    fn saved_settings_replace_the_file_and_load_back() {
        let dir = TempDir::new("save");
        fs::write(dir.file(), "start_with_windows = false").unwrap();
        let settings = parse(VALID).expect("valid");

        save(&dir.file(), &settings).unwrap();

        assert_eq!(load(&dir.file()), (settings, None));
        assert!(!dir.0.join("settings.toml.tmp").exists());
    }
}
