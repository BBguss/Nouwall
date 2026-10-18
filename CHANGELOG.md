# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-18

First public release.

### Added

- Shader wallpapers: Shadertoy-style GLSL and WGSL, rendered with wgpu.
- Video wallpapers with hardware decoding through Media Foundation.
- Web wallpapers (HTML file, folder or URL) through WebView2.
- Hardware probe (GPU, VRAM, hardware decoders, monitors, power) and `low` / `mid` / `high` tiers from `profiles.toml`.
- Warnings before applying a wallpaper the device may not handle well, with a confirmation dialog and `--force` on the CLI.
- Pausing on full-screen apps, session lock and display off; optional pausing on maximized windows and battery.
- Frame-time governor that lowers render scale, then frame rate.
- Settings window with a live preview and per-wallpaper display options (fit, size, position, render resolution).
- Per-monitor wallpapers and a scaled monitor layout view.
- Tray menu and `nouwallctl` command-line companion.
- English and Bahasa Indonesia for the UI, tray, CLI and warnings, following the Windows display language by default.
- App icon embedded in the executables.

[Unreleased]: https://github.com/BBguss/Nouwall/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/BBguss/Nouwall/releases/tag/v0.1.0
