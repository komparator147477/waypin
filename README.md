<div align="center">

# 📌 Waypin

**A sleek clipboard viewer for Wayland/X11 with GTK3**

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![GTK3](https://img.shields.io/badge/GTK-3.0-green.svg)](https://gtk.org)
[![Wayland](https://img.shields.io/badge/Wayland-supported-purple.svg)](https://wayland.freedesktop.org)

*Instantly preview and manage your clipboard content with a beautiful, responsive interface*

</div>

## Running

Just launch `waypin` with no arguments — it reads the clipboard once, shows a preview window, and exits when you close it.

On Wayland, waypin uses `wl-paste`/`wl-copy` from wl-clipboard. In an X11 session
(`DISPLAY` set, no `WAYLAND_DISPLAY`) it uses `xclip` instead, so install `xclip` there.

### Hyprland preview windows

On Hyprland, previews use ordinary windows even when built with `pin-on-top`.
This preserves the title bar, close button, resizing, and compositor window
controls. Layer-shell surfaces bypass window rules and can collapse to the
minimum widget size, so they are unsuitable for these interactive previews.
Add a floating/pinned rule to `hyprland.lua` to keep previews visible:

```lua
hl.window_rule({
    name = "waypin-preview",
    match = { class = "^waypin$" },
    float = true,
    center = true,
    pin = true,
})
```

### Look

Image previews open at the image's aspect ratio (fit to the screen, never
upscaled) with the copy button floating in the corner, and are drawn at the
display's scale, so screenshots stay sharp on HiDPI and fractional scaling.

Preview windows use the GTK theme's background. To restyle them, for example
with a translucent background that the compositor can blur, put CSS in
`~/.config/waypin/style.css` (`$XDG_CONFIG_HOME/waypin/style.css`). Windows
have the `waypin-preview` class:

```css
window.waypin-preview { background-color: rgba(18, 22, 36, 0.70); }
window.waypin-preview scrolledwindow,
window.waypin-preview viewport,
window.waypin-preview textview,
window.waypin-preview textview text { background-color: transparent; }
```

## Background mode (Hyprland global shortcut)

Build with the `global-shortcuts` feature to enable the background daemon:

```bash
cargo build --release --features global-shortcuts
```

Run `waypin --background` to register the `waypin:toggle` global shortcut and stay running. Each shortcut press opens a fresh, independent preview window. The daemon is event-driven and lazily initializes GTK on the first press, so idle RSS sits around 1–2 MB.

Outside Hyprland, or when no Wayland display is available, `waypin --background` prints a `waypin: background shortcut listener failed: ...` error to stderr and exits with status 1. It does not open a preview window. When started by the supplied systemd service, inspect these errors with:

```bash
journalctl --user -u waypin.service -f
```

### Bind the shortcut in `hyprland.lua`

```lua
hl.bind("SUPER + V", hl.dsp.global("waypin:toggle"))
```

### systemd user service (recommended)

A `systemd/waypin.service` unit is provided. The unit self-skips on non-Hyprland sessions via `ConditionPathExists=/usr/bin/Hyprland` and `ConditionEnvironment=XDG_CURRENT_DESKTOP=Hyprland`, so it is safe to install unconditionally.

```bash
# After installing the package (or copying the unit manually to
# /usr/lib/systemd/user/ or ~/.config/systemd/user/):
systemctl --user enable --now waypin.service
```

Resource caps force the daemon into ultra-low-power mode: `MemoryMax=48M`, `Nice=19`, `CPUWeight=20`. The daemon restarts on crash (`Restart=on-failure`).

## Arch Linux package

The package enables both `global-shortcuts` and `pin-on-top`. Pacman prints the Hyprland binding and user-service setup instructions after installation and upgrades. Restart an enabled daemon after an upgrade:

```bash
systemctl --user restart waypin.service
```
