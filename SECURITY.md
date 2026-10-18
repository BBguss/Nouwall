# Security Policy

## Supported versions

Only the latest release receives security fixes.

## Reporting a vulnerability

Please do not open a public issue for security problems.

Report them privately through
[GitHub Security Advisories](https://github.com/BBguss/Nouwall/security/advisories/new).
Include the nouwall version, your Windows version, the steps to reproduce, and the impact you expect.

You should get a reply within 7 days. Once a fix is released, the advisory is
published with credit to you, unless you prefer to stay anonymous.

## Scope

These areas are especially relevant:

- **Web wallpapers** run in WebView2 and can load remote pages. A page escaping
  its sandbox, or reaching the settings window IPC, is in scope.
- **Library files**, such as a crafted `wallpaper.toml`, video or shader that
  reads or writes outside the library folder.
- **The local file server** behind `http://nouwall.localhost/`, which serves the settings UI.

A shader that only uses a lot of GPU time is a performance issue, not a vulnerability.
