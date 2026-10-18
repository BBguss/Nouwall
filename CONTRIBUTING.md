# Contributing to nouwall

Thanks for helping. You can contribute in several ways: GPU profiles, wallpapers,
bug reports, translations, and code. Please follow the
[Code of Conduct](CODE_OF_CONDUCT.md).

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
- Before sending changes, run `cargo fmt`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test -p nouwall-core`. Add user-visible changes to `CHANGELOG.md`
  under **Unreleased**.
- Logs are in English. Everything a user reads (warnings, tier reasons, tray,
  CLI, error messages) is in English and Bahasa Indonesia. In Rust, use
  `tr!("English {x}", "Indonesia {x}", x = ...)` from `nouwall_core`. In the
  settings UI, add the key to both `STRINGS.en` and `STRINGS.id` and reference it
  with `data-i18n="key"` or `t('key')`. Keep messages short and concrete.
- The settings UI (`crates/nouwall/assets/ui/settings.html`) is a single file
  with no build step. It uses inline Lucide SVG icons and no emoji.
- The app icon is generated from the artwork in `docs/assets/brand/`. After
  changing it, run `python3 scripts/make-icons.py` (needs Pillow) and commit the
  generated files under `crates/nouwall/assets/icon/` and `docs/assets/`.

## Translations

A new language needs a `Lang` variant in `crates/nouwall-core/src/i18n.rs`, a third
template in the `tr!` macro, and a `STRINGS` table in the settings UI. Please open
an issue first so the macro change can be discussed.

## Licensing

By contributing, you agree that your work is dual-licensed under MIT and
Apache-2.0, like the rest of the project.
