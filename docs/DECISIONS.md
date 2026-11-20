# Decisions

A superseded decision is marked "superseded by ADR-xxx", never rewritten.

## ADR-001: native Win32 Rust app, in-house HID++ layer, Velopack

- Date: 2026-11-20. Status: accepted.

### Context

palaswitch is a public, very lightweight Windows utility that lives in the notification area, switches the onboard profile of a Logitech mouse reliably and has a single small settings window (global shortcuts, profile names, start with Windows), in French and English. The tool it replaces (an AutoHotkey script calling a Python HID++ library) failed about three times out of four, most likely because its transport took the first frame it read as the answer to its request, while notifications and replies meant for other Logitech software arrive on the same handle.

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
- Unsigned executable: SmartScreen warns on first launch; documented in the README.
- `vpk` needs the .NET 8 SDK on the release machine and in CI.
- The skeleton measured 2.1 MB of private memory and no CPU time at rest (2026-11-20).
