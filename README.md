# palaswitch

A small Windows tray utility that switches the onboard profile of a Logitech gaming mouse, from the tray menu or with a global keyboard shortcut. It talks to the mouse directly over HID++ 2.0 (onboard profiles feature, 0x8100), so it needs neither G HUB nor a script running in the background.

Status: early development. The HID++ layer and a command line tool work; the tray app only shows an icon with a Quit entry so far.

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

The workspace has four crates:

- `palaswitch-hidpp`: the HID++ protocol, with no dependency on Windows or on a HID library, tested against frames captured from a real mouse.
- `palaswitch-device`: finds HID++ devices with hidapi and opens both HID++ collections of each interface.
- `palaswitch-cli`: a command line tool for trying the protocol on a connected mouse.
- `palaswitch`: the tray app.

## Command line tool

```powershell
cargo run --release -p palaswitch-cli -- list         # mice, profiles, active profile
cargo run --release -p palaswitch-cli -- get          # active profile
cargo run --release -p palaswitch-cli -- set 2        # make profile 2 active
cargo run --release -p palaswitch-cli -- sniff 10     # print every HID++ report for 10 s
cargo run --release -p palaswitch-cli -- stress 200   # 200 switches between enabled profiles
```

Add `--pid C095` to pick a device by USB product ID, and `-v` for debug logs. `stress` puts the mouse back on the profile it started on, also when stopped with Ctrl+C.

## License

MIT, see [LICENSE](LICENSE).
