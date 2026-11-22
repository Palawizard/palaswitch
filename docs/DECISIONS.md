# Decisions

A superseded decision is marked "superseded by ADR-xxx", never rewritten.

## ADR-001: native Win32 Rust app, in-house HID++ layer, Velopack

- Date: 2026-11-20. Status: accepted.

### Context

palaswitch is a public, very lightweight Windows utility that lives in the notification area, switches the onboard profile of a Logitech mouse reliably and has a single small settings window (global shortcuts, profile names, start with Windows), in French and English. The tool it replaces (an AutoHotkey script calling a Python HID++ library) failed about three times out of four, most likely because its transport took the first frame it read as the answer to its request, while notifications and replies meant for other HID++ software arrive on the same handle.

### Options considered

1. **Rust + Tauri 2 + React**: quick to polish, installer and updater included. About 4 MB at rest once the window is destroyed according to two uncontrolled Windows measurements; slower window opening while WebView2 starts.
2. **Native Win32 Rust** (official `windows` crate, `tray-icon`, `muda`, `global-hotkey`): no WebView, a few MB at rest and instant window opening expected; the interface is written by hand and dark mode for standard controls is poorly documented.
3. **Slint**: native tray icon since 1.17 and a light software renderer, but a license to manage (badge or About screen under the royalty-free license) and little feedback on Windows.
4. **egui/eframe**: window close bug on Windows in 0.37, a single winit event loop per process, GPU rendering heavy on memory.

For HID++: `hidpp` 0.2.0 (no 0x8100 support), `openlogi-hidpp` 0.9.0 (correct, but its Windows transport pulls tokio and async-hid), `logiops-core` 0.1.0 (wrong function numbers), or our own layer on `hidapi` 2.6.7.

### Decision

- Stable Rust, Cargo workspace: `palaswitch-hidpp` (pure protocol, testable without hardware), `palaswitch-cli` (test tool for a real mouse), `palaswitch` (Win32 app).
- Native Win32 interface with the `windows` crate; tray icon, menu and shortcuts with `tray-icon`, `muda` and `global-hotkey` (`RegisterHotKey`).
- Own HID++ layer on `hidapi`: both HID collections (short and long reports), strict response matching, software ID 0x0E, 0xFF and 0x8F errors, bounded retries.
- Settings in TOML, logs with `tracing`, start with Windows through the `HKCU\...\Run` key.
- Per-user installer and updates from GitHub Releases with Velopack.

### Consequences

- No React, Vite or Node in the project.
- The interface takes more code (controls, dark theme, DPI, keyboard); the window stays deliberately tiny.
- Unsigned executable: SmartScreen warns on first launch; to be explained in the README with the first release.
- `vpk` needs the .NET 8 SDK on the release machine and in CI.
- The skeleton measured 2.1 MB of private memory and no CPU time at rest (2026-11-20).

## ADR-002: one transport per HID++ interface, tray icon without a GUID

- Date: 2026-11-21. Status: accepted.

### Context

Each `HidTransport` runs one reader thread per HID collection, blocked on a read. A transport dropped while its device is still there keeps those threads and their handles until the next report arrives. Discovery used to open one transport per device index behind a receiver and drop the ones where nothing answered.

Failure notifications (`Shell_NotifyIconW` with `NIF_INFO`) must name the existing tray icon, either by GUID or by window and `uID`. Windows binds the GUID of an unsigned executable to its path: a second copy of the app at another path could no longer add its icon.

### Decision

- Discovery keeps the first device with onboard profiles behind each interface and keeps one transport per interface for as long as Windows lists it, also when no mouse answers (a receiver whose mouse is asleep), so that the interface can be probed again on its own traffic.
- The tray icon has no GUID. Notifications address it by the window of `tray-icon` and the `uID` that `tray-icon` 0.26.1 gives the only icon of a process (2). The crate is pinned to that version and the app logs an error at start-up if Windows does not know the icon under that id.

### Consequences

- A receiver with two paired devices that both have onboard profiles only shows the first one.
- Updating `tray-icon` means checking the `uID` again.
- Without a GUID, Windows remembers whether the icon is pinned to the taskbar by executable path; the installer keeps the path stable across updates.

## ADR-003: global-hotkey kept with its release thread

- Date: 2026-11-21. Status: accepted.

### Context

ADR-001 chose `global-hotkey` for the shortcuts. On each press, version 0.10.0 starts a thread that reads `GetAsyncKeyState` every 50 ms until the key is released, to send a release event. The app only uses presses, and its rule is to never poll.

### Decision

Keep `global-hotkey`. The thread lives only while a shortcut is held (a fraction of a second per press), the app does nothing with the release events, and nothing runs at rest.

### Consequences

- No timer and no CPU use at rest, measured with the window closed.
- If this ever matters, registering the shortcuts with `RegisterHotKey` on the app's own hidden window would remove both the thread and the dependency.

## ADR-004: C runtime linked into the exe, update network calls bounded

- Date: 2026-11-22. Status: accepted.

### Context

The first install on a clean Windows Sandbox failed: the exe needed `VCRUNTIME140.dll`, which only comes with the Visual C++ redistributable. Velopack can install the redistributable, but that needs administrator rights, which REL-01 rules out. Separately, Velopack's GitHub source sets no network timeout, so a connection left half open (after sleep, behind a captive portal) could hold the update thread forever.

### Decision

- Link the C runtime statically (`-C target-feature=+crt-static` in `.cargo/config.toml`). `scripts/pack.ps1` refuses to pack an exe that names `vcruntime*.dll` or `api-ms-win-crt-*`.
- Run each update check and download on a thread of its own with a deadline (2 min, 10 min); past it, the call counts as failed and the thread is left behind. The updater is started only after a download that finished in time.

### Consequences

- The exe grows from 4.7 MB to 4.8 MB and depends only on DLLs that ship with Windows.
- A `RUSTFLAGS` variable in the environment replaces the config file; the pack check catches that.
- A network call that never answers leaves one idle thread per try, at most one an hour.
- Updates are trusted on the strength of the GitHub account: the package is checked against the SHA-256 in the release feed, and nothing is signed.
