# Codex Agent Guide

## What This Repo Is
- Wayland notification daemon with GPU rendering and Lua-as-data configuration. Workspace crates: `notiser-daemon` (daemon), `notiser-render` (wgpu renderer), `notiser-types` (shared types/config/layout), `notiser-ctl` (CLI D-Bus client).

## Build & Run
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace`
- `cargo run -p notiser-daemon`
- `cargo run -p notiser-ctl -- <subcommand>`

## Key Entry Points
- Daemon start: `crates/notiser-daemon/src/main.rs` -> `app::run()` wires tracing, Wayland, D-Bus, config, and event loop (calloop).
- D-Bus services: `crates/notiser-daemon/src/dbus` exposes `org.freedesktop.Notifications` + `org.notiser.Daemon`; bridge threads move zbus async events into calloop via channels.
- Rendering: `crates/notiser-daemon/src/wayland` builds the layer-shell surface; `notiser-render` handles wgpu drawing, text via cosmic-text/glyphon, SVG via resvg.
- Layout/Config: Lua file at `~/.config/notiser/init.lua` parsed by `crates/notiser-daemon/src/config/lua.rs` into `notiser_types::config::Config` and optional `LayoutNode` tree.
- Notification lifecycle: `crates/notiser-daemon/src/notification` manages queue/history, grouping/matching, timeouts, close reasons, and actions.

## Runtime Behavior
- Event loop: calloop drives Wayland, timers, signals, config watcher, and bridged D-Bus commands. Animation ticks every frame; a 100ms timer enforces timeouts.
- Surface strategy: single layer-shell surface per output; presentation strategy is swappable (Classic stack vs Dynamic island) inside `presentation` module.
- Do Not Disturb: boolean in `Config.dnd`; CLI `notiser-ctl dnd` toggles it; suppressed notifications are ignored unless marked critical by rules/urgency overrides.
- History: stored when enabled (`config.history.enabled`), capped by `max_entries`; transient notifications can be excluded.

## CLI (notiser-ctl)
- Subcommands: `list`, `close <id>`, `close-all`, `dnd`, `reload [--hard]`, `history --limit N`, `inspect`.
- Output formats: `text` (default) or `json` via `--format json`.

## Hot Reloading
- Config watcher listens to `~/.config/notiser/init.lua`. Soft reload updates styling/behavior; `--hard` rebuilds surfaces/layout. Both go through `ConfigReloadEvent` in calloop.

## Rendering Notes
- Uses wgpu 24; unsafe raw window handle bridge with smithay client toolkit in `wayland/surface.rs`—drop order matters.
- Animations driven by `AnimationController` using preset curves (LUT) from `notiser-types::animation`.

## Development Tips
- Keep clippy clean; workspace lints warn on unsafe code and most pedantic rules.
- Lua sandbox removes `os/io/require`; layout DSL is embedded Lua data, not TOML.
- Use `notify-send` for manual tests; `busctl --user introspect org.freedesktop.Notifications /org/freedesktop/Notifications` to inspect D-Bus.

## Directory Cheat Sheet
- `crates/notiser-daemon/src/app.rs` main loop + state wiring.
- `crates/notiser-daemon/src/wayland/` surface/input/render glue.
- `crates/notiser-daemon/src/presentation/` strategies.
- `crates/notiser-daemon/src/config/` Lua loader + watcher.
- `crates/notiser-daemon/src/notification/` queue, grouping, matching, history.
- `crates/notiser-render/src/` rendering primitives, text, shapes, shaders.
- `crates/notiser-types/src/` config, layout AST, animation presets, notification types.
