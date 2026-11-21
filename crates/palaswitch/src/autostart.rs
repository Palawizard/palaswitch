//! "Start with Windows": a value in the current user's Run key, which needs
//! no administrator rights.

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR};

/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `palaswitch`.
pub const RUN_KEY: RunKey = RunKey {
    subkey: r"Software\Microsoft\Windows\CurrentVersion\Run",
    value: "palaswitch",
    approved_subkey: r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
};

/// A string value under `HKEY_CURRENT_USER`, and where Windows notes that
/// the user turned it off in the Task Manager or the startup apps settings.
#[derive(Debug, Clone, Copy)]
pub struct RunKey {
    pub subkey: &'static str,
    pub value: &'static str,
    pub approved_subkey: &'static str,
}

/// The command Windows runs at sign-in: the quoted path of the executable.
fn command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

impl RunKey {
    /// Whether the value exists, starts this executable, and was not turned
    /// off in the startup apps.
    pub fn is_enabled(self, exe: &Path) -> bool {
        self.points_to(exe) && !self.turned_off()
    }

    /// Whether the value exists and starts this executable.
    fn points_to(self, exe: &Path) -> bool {
        self.read()
            .is_some_and(|command_line| command_line.eq_ignore_ascii_case(&command(exe)))
    }

    /// Makes the value start this executable again if it is missing or
    /// points elsewhere (the app moved), and leaves a switch-off made in the
    /// startup apps as the user left it.
    pub fn restore(self, exe: &Path) -> windows::core::Result<()> {
        if self.points_to(exe) {
            return Ok(());
        }
        self.write_command(exe)
    }

    /// Windows writes a binary value whose first byte is odd when the user
    /// turns the app off in the startup apps; even, or no value, means on.
    fn turned_off(self) -> bool {
        let subkey = HSTRING::from(self.approved_subkey);
        let value = HSTRING::from(self.value);
        let mut data = [0u8; 16];
        let mut size = data.len() as u32;
        // SAFETY: `data` and `size` describe a writable buffer that outlives
        // the call; the names are null-terminated.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_BINARY,
                None,
                Some(data.as_mut_ptr().cast::<c_void>()),
                Some(&mut size),
            )
        };
        status.is_ok() && size > 0 && data[0] & 1 == 1
    }

    /// Ticks or unticks the box. Ticking also undoes a switch-off made in
    /// the startup apps, which would otherwise keep the app from starting.
    pub fn set(self, exe: &Path, enabled: bool) -> windows::core::Result<()> {
        if enabled {
            self.write_command(exe)?;
            delete_value(self.approved_subkey, self.value)
        } else {
            delete_value(self.subkey, self.value)
        }
    }

    fn write_command(self, exe: &Path) -> windows::core::Result<()> {
        let subkey = HSTRING::from(self.subkey);
        let value = HSTRING::from(self.value);
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
        .ok()
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

/// Deletes a value under HKCU; a value already gone is not an error.
fn delete_value(subkey: &str, value: &str) -> windows::core::Result<()> {
    let subkey = HSTRING::from(subkey);
    let value = HSTRING::from(value);
    // SAFETY: the names are null-terminated and outlive the call.
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &subkey, &value) } {
        ERROR_FILE_NOT_FOUND => WIN32_ERROR(0),
        status => status,
    }
    .ok()
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::Registry::RegDeleteTreeW;

    use super::*;

    /// Keys of their own under HKCU, deleted on drop.
    struct TestKey(RunKey);

    impl TestKey {
        fn new(subkey: &'static str, approved_subkey: &'static str) -> Self {
            Self(RunKey {
                subkey,
                value: "palaswitch",
                approved_subkey,
            })
        }
    }

    impl Drop for TestKey {
        fn drop(&mut self) {
            for key in [self.0.subkey, self.0.approved_subkey] {
                let subkey = HSTRING::from(key);
                // SAFETY: deletes a test key and its values only.
                let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &subkey) };
            }
        }
    }

    #[test]
    fn enabling_writes_the_quoted_path_and_disabling_removes_it() {
        let key = TestKey::new(
            r"Software\palaswitch-test-autostart-1",
            r"Software\palaswitch-test-autostart-1-approved",
        );
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
    fn a_switch_off_in_the_startup_apps_is_kept_until_the_box_is_ticked() {
        let key = TestKey::new(
            r"Software\palaswitch-test-autostart-3",
            r"Software\palaswitch-test-autostart-3-approved",
        );
        let exe = Path::new(r"C:\palaswitch\palaswitch.exe");
        key.0.set(exe, true).unwrap();
        // What Windows writes when the app is turned off: 03, then a time.
        let off = [3u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let subkey = HSTRING::from(key.0.approved_subkey);
        let value = HSTRING::from(key.0.value);
        // SAFETY: writes a binary value of the test key from a local buffer.
        let written = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &subkey,
                &value,
                windows::Win32::System::Registry::REG_BINARY.0,
                Some(off.as_ptr().cast::<c_void>()),
                off.len() as u32,
            )
        };
        written.ok().unwrap();

        let turned_off = key.0.is_enabled(exe);
        key.0.restore(exe).unwrap();
        let after_restore = key.0.is_enabled(exe);
        key.0.set(exe, true).unwrap();

        assert!(!turned_off);
        assert!(!after_restore, "restoring undid the switch-off");
        assert!(key.0.is_enabled(exe));
    }

    #[test]
    fn restoring_points_the_value_to_the_moved_exe() {
        let key = TestKey::new(
            r"Software\palaswitch-test-autostart-4",
            r"Software\palaswitch-test-autostart-4-approved",
        );
        let exe = Path::new(r"C:\palaswitch\new\palaswitch.exe");
        key.0
            .set(Path::new(r"C:\palaswitch\old\palaswitch.exe"), true)
            .unwrap();

        key.0.restore(exe).unwrap();

        assert!(key.0.is_enabled(exe));
    }

    #[test]
    fn disabling_twice_is_not_an_error_and_another_exe_does_not_count() {
        let key = TestKey::new(
            r"Software\palaswitch-test-autostart-2",
            r"Software\palaswitch-test-autostart-2-approved",
        );
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
