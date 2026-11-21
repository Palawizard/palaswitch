//! "Start with Windows": a value in the current user's Run key, which needs
//! no administrator rights.

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR};

/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `palaswitch`.
pub const RUN_KEY: RunKey = RunKey {
    subkey: r"Software\Microsoft\Windows\CurrentVersion\Run",
    value: "palaswitch",
};

/// A string value under `HKEY_CURRENT_USER`.
#[derive(Debug, Clone, Copy)]
pub struct RunKey {
    pub subkey: &'static str,
    pub value: &'static str,
}

/// The command Windows runs at sign-in: the quoted path of the executable.
fn command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

impl RunKey {
    /// Whether the value exists and starts this executable.
    pub fn is_enabled(self, exe: &Path) -> bool {
        self.read()
            .is_some_and(|command_line| command_line.eq_ignore_ascii_case(&command(exe)))
    }

    pub fn set(self, exe: &Path, enabled: bool) -> windows::core::Result<()> {
        let subkey = HSTRING::from(self.subkey);
        let value = HSTRING::from(self.value);
        let status = if enabled {
            let data: Vec<u16> = command(exe)
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            // SAFETY: `data` is a null-terminated UTF-16 string whose size in
            // bytes is passed along; the names outlive the call.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    &subkey,
                    &value,
                    REG_SZ.0,
                    Some(data.as_ptr().cast::<c_void>()),
                    (data.len() * 2) as u32,
                )
            }
        } else {
            // SAFETY: the names are null-terminated and outlive the call.
            match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &subkey, &value) } {
                ERROR_FILE_NOT_FOUND => WIN32_ERROR(0),
                status => status,
            }
        };
        status.ok()
    }

    fn read(self) -> Option<String> {
        let subkey = HSTRING::from(self.subkey);
        let value = HSTRING::from(self.value);
        let mut buffer = [0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` and `size` describe a writable buffer that
        // outlives the call; the names are null-terminated.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                Some(&mut size),
            )
        };
        if status.is_err() {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buffer[..len]))
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::Registry::RegDeleteTreeW;

    use super::*;

    /// A key of its own under HKCU, deleted on drop.
    struct TestKey(RunKey);

    impl TestKey {
        fn new(subkey: &'static str) -> Self {
            Self(RunKey {
                subkey,
                value: "palaswitch",
            })
        }
    }

    impl Drop for TestKey {
        fn drop(&mut self) {
            let subkey = HSTRING::from(self.0.subkey);
            // SAFETY: deletes the test key and its values only.
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &subkey) };
        }
    }

    #[test]
    fn enabling_writes_the_quoted_path_and_disabling_removes_it() {
        let key = TestKey::new(r"Software\palaswitch-test-autostart-1");
        let exe = Path::new(r"C:\Users\Someone\AppData\Local\palaswitch\current\palaswitch.exe");

        let before = key.0.is_enabled(exe);
        key.0.set(exe, true).unwrap();
        let written = key.0.read();
        let enabled = key.0.is_enabled(exe);
        key.0.set(exe, false).unwrap();
        let disabled = key.0.is_enabled(exe);

        assert!(!before && enabled && !disabled);
        assert_eq!(written, Some(format!("\"{}\"", exe.display())));
    }

    #[test]
    fn disabling_twice_is_not_an_error_and_another_exe_does_not_count() {
        let key = TestKey::new(r"Software\palaswitch-test-autostart-2");
        let exe = Path::new(r"C:\palaswitch\palaswitch.exe");
        key.0
            .set(Path::new(r"C:\elsewhere\palaswitch.exe"), true)
            .unwrap();

        let other = key.0.is_enabled(exe);

        assert!(!other);
        key.0.set(exe, false).unwrap();
        key.0.set(exe, false).unwrap();
    }
}
