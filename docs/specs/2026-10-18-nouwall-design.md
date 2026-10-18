# nouwall — Live Wallpaper Engine untuk Windows (Rust) — Desain

Tanggal: 2026-10-18 · Status: diimplementasikan (opsi 4, seluruh bagian A–F)

## Tujuan

Aplikasi live wallpaper open source untuk Windows 10/11 yang ditulis dalam Rust.
Mendukung tiga jenis wallpaper (shader, video, web), berjalan di belakang ikon
desktop, hemat daya, dan **sadar kemampuan hardware**: aplikasi mendeteksi
GPU/decoder/monitor, menentukan tier, dan memberi peringatan bila wallpaper
melebihi kemampuan perangkat (mis. video 4K di iGPU lama).

Non-tujuan (v1): editor wallpaper, workshop online, audio-reactive, macOS/Linux.

## Bagian

| ID | Bagian | Ringkasan |
|----|--------|-----------|
| A | Core desktop + tray | Jendela per monitor ditempel di belakang ikon (WorkerW / Progman 24H2), tray icon, single instance |
| B | Renderer shader | `wgpu`, shader GLSL gaya Shadertoy (`mainImage`) atau WGSL, uniform `iTime`, `iResolution`, `iMouse`, `iFrame` |
| C | Renderer video | Media Foundation `IMFMediaEngine` mode frame-server → tekstur D3D11 → swapchain, loop, mute, hardware decode |
| D | Renderer web | WebView2 (`wry`), file HTML lokal atau URL |
| E | Pengaturan & library | Jendela pengaturan berbasis WebView2 (HTML tertanam), library di `%APPDATA%\nouwall\library`, impor file, config TOML |
| F | Hardware profiler | Probe DXGI/D3D11 video decoder/monitor/CPU/RAM/baterai, klasifikasi tier dari `profiles.toml`, peringatan + auto-downgrade |

## Arsitektur

Workspace Cargo dengan dua crate:

- **`nouwall-core`** (lintas platform, diuji di Linux/amba): tipe data dan logika murni
  - `config` — `Config` (TOML), penugasan wallpaper per monitor, opsi umum
  - `manifest` — `wallpaper.toml` per wallpaper (`kind = shader|video|web`, entry, metadata)
  - `hardware` — `HardwareInfo` (GPU, VRAM, decoder caps per codec, monitor, CPU, RAM, power)
  - `tier` — `classify(&HardwareInfo, &Profiles) -> TierReport` (Low/Mid/High + budget)
  - `policy` — `evaluate(&Manifest, &TierReport, &Monitor) -> Vec<Warning>`
  - `pause` — keputusan pause/resume dari kondisi (fullscreen, baterai, sesi terkunci)
  - `governor` — frame-time governor: turunkan fps/render-scale bila lambat
- **`nouwall`** (binary Windows): Win32 + rendering
  - `desktop` — temukan WorkerW / Progman, parenting, koordinat monitor
  - `probe` — isi `HardwareInfo` lewat DXGI, `ID3D11VideoDevice` (cek decode 4K per profil), `GetSystemPowerStatus`, `EnumDisplaySettings`
  - `render::{shader, video, web}` — implementasi trait `Renderer`
  - `tray`, `settings` (WebView2 + IPC JSON), `media_info` (resolusi/codec video via `IMFSourceReader`), `autostart` (HKCU Run)

Event loop: Win32 murni (tanpa `winit`). Jendela controller tersembunyi menerima
`WM_COPYDATA` (perintah dari `nouwallctl`/instance kedua), `WM_DISPLAYCHANGE`,
`TaskbarCreated`, notifikasi sesi terkunci (WTS) dan status layar
(`GUID_CONSOLE_DISPLAY_STATE`). Penantian memakai `MsgWaitForMultipleObjectsEx` + waitable
timer resolusi tinggi untuk tenggat frame. Satu `Host` per monitor = jendela child borderless
di WorkerW (atau di Progman pada 24H2) + satu `Renderer`; host dibuat ulang setiap renderer
berganti (swapchain DXGI terikat ke HWND). Tick 500 ms memeriksa pause policy; `config.toml`
dipantau via mtime (~2 s), sehingga `nouwallctl apply` dari sesi lain (mis. SSH) tetap
termuat.

## Tier & peringatan (F)

Tier dihitung dari `profiles.toml` (data, bisa ditambah kontributor) + heuristik:

| Tier | Contoh | Budget default |
|------|--------|----------------|
| Low | Intel HD 5xx/6xx, UHD 6xx, adapter software | video ≤ 1920×1080@30, shader 30 fps, render scale 0.75 |
| Mid | Iris Xe, Radeon Vega/680M iGPU, GPU diskrit < 4 GB | video ≤ 2560×1440@60, shader 60 fps |
| High | GPU diskrit ≥ 4 GB VRAM | video ≤ 3840×2160@60, shader 60 fps |

Penyesuaian: CPU < 4 thread → turun satu tier; baterai → fps cap dibagi dua (opsional pause).
`tier_override` di config menimpa hasil deteksi.

Jenis peringatan saat apply:
- **Error-ish** `NoHardwareDecode` — codec/resolusi tak didukung decoder hardware (software decode, CPU tinggi)
- **Warn** `ExceedsTier` — "Perangkat Anda mungkin tidak mendukung 4K dengan lancar"
- **Info** `Downscaled` — resolusi sumber > resolusi monitor (boros daya)
- **Warn** `HeavyShader` — shader di tier Low (render scale diturunkan otomatis)

Peringatan tidak memblokir: UI menawarkan *Tetap pakai* / *Batal*. Governor runtime menurunkan
fps/render scale shader bila frame time > 1.5× budget selama 3 detik, dan mencatat ke log.

## Pause policy

Pause per monitor bila: jendela foreground fullscreen/maximized menutupi monitor itu
(opsi), perangkat memakai baterai (opsi), sesi terkunci / layar mati. Shader berhenti
render, video `Pause()`, web disembunyikan.

## Data & file

- `%APPDATA%\nouwall\config.toml`, `%APPDATA%\nouwall\profiles.toml` (opsional override)
- `%APPDATA%\nouwall\library\<id>\wallpaper.toml` + aset
- `%LOCALAPPDATA%\nouwall\nouwall.log`
- Sampel bawaan (shader + web) tertanam di binary; sampel video ikut di zip rilis `samples\`

## CLI

`nouwall.exe` (GUI, tray) dan `nouwallctl.exe` (konsol): `probe [--json]` · `list` ·
`apply <id> [--monitor N] [--force]` (exit 3 bila ditahan peringatan) · `import <file|folder>` ·
`settings | pause | resume | toggle | reload | quit` · `paths` · `capture [folder]`
(PrintWindow jendela wallpaper + pengaturan → BMP, untuk diagnostik)

## Build & uji

- Build cross di amba: `cargo xwin build --release --target x86_64-pc-windows-msvc`
- Unit test `nouwall-core` di amba (Linux native)
- Deploy ke mbb lewat NAS (scp), jalankan via Scheduled Task `/IT` di sesi interaktif,
  verifikasi dengan screenshot + log
- CI GitHub Actions (`windows-latest`) untuk kontributor

## Penanganan error

Tiap renderer gagal → log + fallback ke warna solid, aplikasi tetap hidup. WorkerW tidak
ditemukan → coba ulang tiap 2 detik (Explorer restart ditangani via `TaskbarCreated`).
Config rusak → backup ke `config.toml.bak`, pakai default.

## Lisensi

MIT OR Apache-2.0.
