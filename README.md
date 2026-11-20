# palaswitch

A small Windows tray utility that switches the onboard profile of a Logitech gaming mouse, from the tray menu or with a global keyboard shortcut. It talks to the mouse directly over HID++ 2.0 (onboard profiles feature, 0x8100), so it needs neither G HUB nor a script running in the background.

Status: early development. The current build only shows a tray icon with a Quit entry; profile switching is not there yet.

Developed against a G502 X PLUS (wired and LIGHTSPEED). Other Logitech mice that keep profiles in onboard memory are expected to work, but they have not been tried.

## Installation

There is no release yet. To build from source on Windows 10 or 11:

1. Install Rust stable with the MSVC toolchain (https://rustup.rs) and the Visual Studio C++ Build Tools.
2. Clone the repository and build:

   ```powershell
   git clone https://github.com/Palawizard/palaswitch.git
   cd palaswitch
   cargo build --release
   ```

The app is `target\release\palaswitch.exe`.

## Usage

Run `palaswitch.exe`. An icon appears in the notification area; click it and choose Quit to close the app.

The interface follows the Windows display language: French if Windows is in French, English otherwise.

## Development

Run every check (format, lint, tests, release build) with:

```powershell
pwsh scripts/verify.ps1
```

The CI runs the same script.

The workspace has three crates:

- `palaswitch-hidpp`: the HID++ protocol, with no dependency on Windows or on a HID library, tested against frames captured from a real mouse.
- `palaswitch-cli`: placeholder for now; it will become a command line tool for trying the protocol on a connected mouse.
- `palaswitch`: the tray app.

## License

MIT, see [LICENSE](LICENSE).
