# Contributing to nouwall

Thanks for helping. You can contribute in several ways: GPU profiles, wallpapers,
bug reports, and code.

## Adding or fixing a GPU profile

The tier table only works if it knows real hardware. If nouwall puts your
laptop in the wrong tier, please send a profile:

1. Run `nouwallctl probe` and copy the output.
2. Try a few wallpapers: a 1080p video, a 4K video if you have one, and the
   built-in shaders. Watch the CPU and GPU in Task Manager, and notice whether
   the fans spin up.
3. Add or edit a `[[gpu]]` entry in
   [`crates/nouwall-core/profiles.toml`](crates/nouwall-core/profiles.toml):

   ```toml
   [[gpu]]
   match = ["Radeon 780M"]      # case-insensitive substrings of the DXGI name
   tier = "mid"                 # low | mid | high
   note = "4K60 HEVC smooth, shaders at 60 fps on AC"
   ```

   Entries are checked top to bottom and the first match wins. Put specific
   names before broad ones.
4. Run `cargo test -p nouwall-core`. Then open a pull request with your probe
   output in the description.

You can test a profile locally without building anything. Copy `profiles.toml`
to `%APPDATA%\nouwall\profiles.toml`, edit it, and restart nouwall. The user file
takes precedence over the built-in one.

## Reporting bugs

Include:

- `nouwallctl probe` output,
- your Windows version (`winver`),
- the end of `%LOCALAPPDATA%\nouwall\nouwall.log` (run `nouwall --verbose` for more detail),
- for rendering problems, the output of `nouwallctl capture` (BMP files of the
  wallpaper windows).

## Code

- `nouwall-core` must stay platform-independent and fully unit-tested. Logic
  that can be tested without Windows (tiers, warnings, pause rules, governor,
  config, library) belongs there.
- `nouwall` holds the Windows-only code: Win32, Direct3D, Media Foundation,
  WebView2. Keep the `unsafe` blocks small and add a comment when an API
  behaves in a surprising way.
- Before sending changes, run `cargo fmt`, `cargo clippy --all-targets`, and
  `cargo test -p nouwall-core`.
- User-facing text is in Bahasa Indonesia for now. Keep messages short and
  concrete.
- The settings UI (`crates/nouwall/assets/ui/settings.html`) is a single file
  with no build step. It uses inline Lucide SVG icons and no emoji.

## Licensing

By contributing, you agree that your work is dual-licensed under MIT and
Apache-2.0, like the rest of the project.
