# Notiser

GPU-accelerated Wayland notification daemon with Lua configuration and Dynamic Island-style morphing.

> **Warning: This project is barely functional and under heavy development.**
> Expect breaking changes, missing features, and rough edges. Not ready for daily use.

## Features

- **GPU-rendered** via wgpu — smooth animations, minimal CPU usage
- **Lua configuration** — neovim-style `init.lua`, no TOML/YAML
- **Dynamic Island mode** — notifications morph and merge into a single surface
- **Classic stack mode** — traditional stacked notification layout
- **Hot reload** — edit your config, see changes instantly (soft) or rebuild surfaces (`--hard`)
- **D-Bus native** — implements `org.freedesktop.Notifications` + custom `org.notiser.Daemon` interface
- **CLI control** — `notiser-ctl` for listing, closing, history, DND, and more
- **Do Not Disturb** — suppress notifications with urgency/rule overrides
- **Notification history** — configurable retention with transient exclusion
- **Per-output surfaces** — one layer-shell surface per output

## Requirements

- Rust 1.85+
- Wayland compositor with layer-shell support (targets [Niri](https://github.com/YaLTeR/niri) and [Hyprland](https://hyprland.org/))
- GPU drivers compatible with wgpu

## Building

```bash
cargo build --workspace            # Build all crates
cargo build --workspace --release  # Release build (LTO + stripped)
```

## Running

```bash
cargo run -p notiser-daemon   # Start the notification daemon
```

## CLI Usage

```bash
notiser-ctl list                  # List active notifications
notiser-ctl close <id>            # Close a notification by ID
notiser-ctl close-all             # Close all notifications
notiser-ctl dnd                   # Toggle Do Not Disturb
notiser-ctl reload [--hard]       # Reload config (--hard rebuilds surfaces)
notiser-ctl history [--limit N]   # Show notification history
notiser-ctl inspect               # Show daemon status
```

All commands support `--format json` for machine-readable output.

## Configuration

Config lives at `~/.config/notiser/init.lua`. Lua-as-data style — define your layout, styling, and behavior in Lua tables. The sandbox removes `os`, `io`, and `require` for security.

## Workspace Structure

| Crate | Description |
|---|---|
| `notiser-daemon` | Main daemon binary (calloop, SCTK, zbus) |
| `notiser-render` | GPU rendering engine (wgpu, cosmic-text, resvg) |
| `notiser-types` | Shared types (config, layout AST, animations) |
| `notiser-ctl` | CLI control tool (clap, zbus client) |

## License

[GPL-3.0-or-later](https://www.gnu.org/licenses/gpl-3.0.html)
