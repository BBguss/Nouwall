# nouwall

Live wallpapers for Windows 10 and 11, written in Rust. One small engine runs
three kinds of wallpaper behind your desktop icons:

| Kind   | Format                                   | Runs on                                   |
|--------|------------------------------------------|-------------------------------------------|
| Shader | Shadertoy-style GLSL (`.glsl`, `.frag`) or WGSL (`.wgsl`) | wgpu (DirectX 12, falls back to Vulkan/OpenGL) |
| Video  | `.mp4`, `.m4v`, `.mkv`, `.webm`, `.mov`, `.wmv`, `.avi` | Media Foundation with hardware decoding   |
| Web    | an HTML file, a folder with `index.html`, or a URL | WebView2 (built into Windows 11)          |

A tray icon switches wallpapers and pauses them. The settings window manages the
library, the per-monitor assignment, and the options.

> Bahasa Indonesia: lihat [bagian di bawah](#bahasa-indonesia).

## Hardware-aware by design

nouwall checks the machine before applying a wallpaper, so a 4K video doesn't
quietly drain a thin laptop's battery. At startup it probes:

- the GPU (name, vendor, VRAM, integrated vs. discrete),
- which video codecs the GPU can decode in hardware, and up to what resolution (H.264, HEVC, VP9, AV1),
- every monitor's resolution and refresh rate,
- whether the machine is on battery.

The GPU is mapped to a **tier** (`low`, `mid`, `high`) through
[`profiles.toml`](crates/nouwall-core/profiles.toml). Each tier has a budget:

| Tier | Video up to | Shader fps | Render scale | Max shader cost |
|------|-------------|-----------:|-------------:|----------------:|
| low  | 1920x1080 @ 30 | 30 | 75% | 2 |
| mid  | 2560x1440 @ 60 | 60 | 100% | 3 |
| high | 3840x2160 @ 60 | 60 | 100% | 5 |

When a wallpaper exceeds the budget, for example a 4K HEVC video on an
Intel HD 620, nouwall shows a warning such as *"Perangkat Anda mungkin tidak
mendukung 4K dengan lancar"* and asks for confirmation. Warnings never block
you: confirm in the UI, or pass `--force` on the command line. A codec with no
hardware decoder at all (for example AV1 on older GPUs) is marked critical.

While running, a frame-time governor lowers the shader render scale, and then
the frame rate, when the GPU can't keep up. Wallpapers pause when a fullscreen
app covers the monitor, the session is locked, or the display is off.
Pausing on a maximized window or on battery is optional.

You can override the tier in Settings → Hardware, or with `tier_override` in
`config.toml`.

## Install

Download `nouwall-<version>-windows-x64.zip` from the releases page and extract
it anywhere. Run `nouwall.exe`, and the tray icon appears. Video and web wallpapers
need the WebView2 runtime and Media Foundation. Both ship with Windows 11. On
Windows 10 N editions, install the Media Feature Pack.

Files the app uses:

| What | Where |
|------|-------|
| Config, GPU profile override, library | `%APPDATA%\nouwall\` |
| Log, WebView2 data | `%LOCALAPPDATA%\nouwall\` |

## Command line

`nouwallctl.exe` is the console companion. It talks to the running engine.

```text
nouwallctl probe [--json]            hardware, tier and decode capabilities
nouwallctl list                      wallpapers in the library (* = active)
nouwallctl apply <id> [--monitor N] [--force]
nouwallctl import <file|folder>      add a video, shader or web page
nouwallctl settings | pause | resume | toggle | reload | quit
nouwallctl paths                     where config, library and log live
nouwallctl capture [folder]          save wallpaper windows as BMP (diagnostics)
```

`apply` exits with code 3 when warnings stop it and `--force` wasn't given.

## Making wallpapers

Every library entry is a folder with a `wallpaper.toml`:

```toml
title = "Aurora"
kind = "shader"          # shader | video | web
entry = "aurora.glsl"    # file inside the folder, or a URL for web
author = "you"
description = "Moving aurora bands"
shader_cost = 2          # 1 (light) .. 5 (heavy); compared with the tier budget
```

Importing a file creates this folder for you. For videos, nouwall fills in the
`[video]` section (resolution, fps, codec) itself.

**GLSL** works like Shadertoy: write `void mainImage(out vec4 fragColor, in vec2 fragCoord)`
and use `iResolution`, `iTime`, `iTimeDelta`, `iFrame`, `iMouse` and `iDate`.
Texture channels (`iChannel0..3`) aren't supported yet.

**WGSL** shaders define `fn main_image(frag_coord: vec2<f32>) -> vec4<f32>` and
read the uniform `u` (`u.resolution`, `u.time`, `u.mouse`, `u.time_delta`,
`u.frame`, `u.date`). See [`starfield.wgsl`](crates/nouwall/assets/shaders/starfield.wgsl).

**Web** wallpapers are normal pages. Audio is muted unless you turn sound on.
The page is paused while hidden, so use `requestAnimationFrame` or
listen to `visibilitychange` instead of running timers forever.

## Building

The workspace has two crates:

- `nouwall-core`: platform-independent logic (config, manifests, library, tiers,
  warning policy, pause rules, governor). It builds and tests anywhere:
  `cargo test -p nouwall-core`.
- `nouwall`: the Windows engine. It builds two binaries: `nouwall` (GUI, tray) and `nouwallctl` (console).

On Windows:

```powershell
cargo build --release
```

Cross-compiling from Linux works with [cargo-xwin](https://github.com/rust-cross/cargo-xwin):

```sh
cargo xwin build --release --target x86_64-pc-windows-msvc
```

`scripts/dist.sh` packs the binaries, licenses and README into a zip.

## How it attaches to the desktop

nouwall sends the undocumented message `0x052C` to `Progman`, which makes
Explorer create a `WorkerW` window between the wallpaper and the icons. Before
Windows 11 24H2 that `WorkerW` is a top-level window, and wallpapers become its
children. On 24H2 and later it is a child of `Progman`, so nouwall places a
layered child window just above it. When nouwall exits, it re-applies your static
wallpaper.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option. Contributions are accepted under the same terms. See
[CONTRIBUTING.md](CONTRIBUTING.md).

---

## Bahasa Indonesia

nouwall adalah aplikasi live wallpaper untuk Windows 10/11 yang ditulis dengan
Rust. Ia menjalankan wallpaper **shader** (GLSL/WGSL), **video** (decode
hardware lewat Media Foundation), dan **web** (HTML lewat WebView2) di belakang
ikon desktop.

Sebelum memasang wallpaper, nouwall memeriksa GPU, kemampuan decode codec,
monitor, dan status baterai, lalu mengelompokkan perangkat ke tier `low`, `mid`,
atau `high`. Wallpaper yang melebihi kemampuan perangkat, misalnya video 4K di
laptop dengan Intel HD 620, memunculkan peringatan dan meminta konfirmasi.
Peringatan tidak pernah memblokir: konfirmasi di jendela pengaturan, atau
pakai `--force` di `nouwallctl`.

Cara cepat:

1. Ekstrak zip rilis, lalu jalankan `nouwall.exe`. Ikon tray akan muncul.
2. Klik kanan ikon tray, lalu pilih **Buka nouwall** untuk membuka library.
3. **Impor file** (video, `.glsl`, `.wgsl`, `.html`), **Impor folder HTML**, atau **Tambah URL**.
4. Pilih monitor, lalu tekan **Pasang**.

Untuk menambahkan profil GPU laptop Anda, jalankan `nouwallctl probe` lalu ikuti
[CONTRIBUTING.md](CONTRIBUTING.md).
