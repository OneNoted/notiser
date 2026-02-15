use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use mlua::prelude::*;
use tracing::{info, warn};

use notiser_types::config::*;
use notiser_types::notification::Urgency;

/// Convert mlua::Error to anyhow::Error (LuaError is !Send so can't use ? directly)
fn lua_err(e: mlua::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

/// Load configuration from `~/.config/notiser/init.lua`.
/// Falls back to defaults if the file doesn't exist.
pub fn load_config() -> Result<Config> {
    let config_path = config_path();
    if !config_path.exists() {
        info!("no config file found at {}, using defaults", config_path.display());
        return Ok(Config::default());
    }

    info!(path = %config_path.display(), "loading config");
    let source = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;

    load_config_from_str(&source)
}

/// Load config from a Lua source string (useful for testing and reload).
pub fn load_config_from_str(source: &str) -> Result<Config> {
    let lua = Lua::new();

    // Sandbox: remove dangerous modules
    sandbox_lua(&lua)?;

    // Set up the notiser module with setup() function
    let config_table: std::sync::Arc<std::sync::Mutex<Option<LuaTable>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));

    let captured = config_table.clone();
    let setup_fn = lua.create_function(move |lua, table: LuaTable| {
        // Clone the table into our capture
        let cloned = lua.create_table()?;
        for pair in table.pairs::<LuaValue, LuaValue>() {
            let (k, v) = pair?;
            cloned.set(k, v)?;
        }
        *captured.lock().unwrap() = Some(cloned);
        Ok(())
    }).map_err(lua_err)?;

    let notiser_module = lua.create_table().map_err(lua_err)?;
    notiser_module.set("setup", setup_fn).map_err(lua_err)?;
    lua.globals().set("notiser", notiser_module).map_err(lua_err)?;

    // Execute the config file
    lua.load(source)
        .set_name("init.lua")
        .exec()
        .map_err(|e| anyhow::anyhow!("failed to execute init.lua: {e}"))?;

    // Extract the config table
    let table = config_table.lock().unwrap().take();
    match table {
        Some(t) => parse_config_table(&lua, &t),
        None => {
            warn!("notiser.setup() was not called in init.lua, using defaults");
            Ok(Config::default())
        }
    }
}

fn config_path() -> PathBuf {
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{home}/.config")
        });
    PathBuf::from(xdg).join("notiser").join("init.lua")
}

fn sandbox_lua(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    for name in &["os", "io", "debug", "loadfile", "dofile"] {
        globals.set(*name, LuaValue::Nil).map_err(lua_err)?;
    }
    Ok(())
}

/// Parse the Lua table from notiser.setup({...}) into a Config struct.
fn parse_config_table(_lua: &Lua, table: &LuaTable) -> Result<Config> {
    let mut config = Config::default();

    if let Ok(general) = table.get::<LuaTable>("general") {
        parse_general(&general, &mut config.general)?;
    }

    if let Ok(display) = table.get::<LuaTable>("display") {
        parse_display(&display, &mut config.display)?;
    }

    if let Ok(appearance) = table.get::<LuaTable>("appearance") {
        parse_appearance(&appearance, &mut config.appearance)?;
    }

    if let Ok(animations) = table.get::<LuaTable>("animations") {
        parse_animations(&animations, &mut config.animations)?;
    }

    if let Ok(urgency) = table.get::<LuaTable>("urgency") {
        parse_urgency_overrides(&urgency, &mut config.urgency)?;
    }

    if let Ok(dnd) = table.get::<LuaTable>("dnd") {
        parse_dnd(&dnd, &mut config.dnd)?;
    }

    if let Ok(history) = table.get::<LuaTable>("history") {
        parse_history(&history, &mut config.history)?;
    }

    if let Ok(audio) = table.get::<LuaTable>("audio") {
        parse_audio(&audio, &mut config.audio)?;
    }

    if let Ok(actions) = table.get::<LuaTable>("actions") {
        parse_actions(&actions, &mut config.actions)?;
    }

    Ok(config)
}

// --- Section parsers ---

fn parse_general(t: &LuaTable, cfg: &mut GeneralConfig) -> Result<()> {
    if let Ok(v) = t.get::<u32>("max_visible") { cfg.max_visible = v; }
    if let Ok(v) = t.get::<u32>("default_timeout") { cfg.default_timeout = v; }
    if let Ok(v) = t.get::<String>("sort_order") {
        cfg.sort_order = match v.as_str() {
            "time_ascending" | "time_asc" => SortOrder::TimeAscending,
            "time_descending" | "time_desc" => SortOrder::TimeDescending,
            "urgency_descending" | "urgency_desc" => SortOrder::UrgencyDescending,
            _ => {
                warn!(value = v, "unknown sort_order, using default");
                cfg.sort_order
            }
        };
    }
    if let Ok(v) = t.get::<u32>("idle_threshold") { cfg.idle_threshold = v; }
    Ok(())
}

fn parse_display(t: &LuaTable, cfg: &mut DisplayConfig) -> Result<()> {
    if let Ok(v) = t.get::<String>("anchor") {
        cfg.anchor = match v.as_str() {
            "top-left" | "top_left" => Anchor::TopLeft,
            "top-center" | "top_center" => Anchor::TopCenter,
            "top-right" | "top_right" => Anchor::TopRight,
            "bottom-left" | "bottom_left" => Anchor::BottomLeft,
            "bottom-center" | "bottom_center" => Anchor::BottomCenter,
            "bottom-right" | "bottom_right" => Anchor::BottomRight,
            "center-left" | "center_left" => Anchor::CenterLeft,
            "center-right" | "center_right" => Anchor::CenterRight,
            _ => {
                warn!(value = v, "unknown anchor, using default");
                cfg.anchor
            }
        };
    }
    if let Ok(v) = t.get::<u32>("gap") { cfg.gap = v; }
    if let Ok(v) = t.get::<String>("layer") {
        cfg.layer = match v.as_str() {
            "background" => Layer::Background,
            "bottom" => Layer::Bottom,
            "top" => Layer::Top,
            "overlay" => Layer::Overlay,
            _ => {
                warn!(value = v, "unknown layer, using default");
                cfg.layer
            }
        };
    }
    if let Ok(v) = t.get::<LuaTable>("margin") {
        parse_margins(&v, &mut cfg.margin)?;
    }
    Ok(())
}

fn parse_margins(t: &LuaTable, m: &mut Margins) -> Result<()> {
    if let Ok(v) = t.get::<u32>("top") { m.top = v; }
    if let Ok(v) = t.get::<u32>("right") { m.right = v; }
    if let Ok(v) = t.get::<u32>("bottom") { m.bottom = v; }
    if let Ok(v) = t.get::<u32>("left") { m.left = v; }
    Ok(())
}

fn parse_appearance(t: &LuaTable, cfg: &mut AppearanceConfig) -> Result<()> {
    if let Ok(v) = t.get::<u32>("width") { cfg.width = v; }
    if let Ok(v) = t.get::<f32>("opacity") { cfg.opacity = v.clamp(0.0, 1.0); }
    if let Ok(v) = t.get::<String>("background") { cfg.background = Color::hex(&v); }
    if let Ok(v) = t.get::<u32>("icon_size") { cfg.icon_size = v; }
    if let Ok(v) = t.get::<String>("icon_theme") { cfg.icon_theme = Some(v); }

    if let Ok(v) = t.get::<LuaTable>("border") {
        parse_border(&v, &mut cfg.border)?;
    }
    if let Ok(v) = t.get::<LuaTable>("padding") {
        parse_margins(&v, &mut cfg.padding)?;
    }
    if let Ok(v) = t.get::<LuaTable>("font") {
        parse_font(&v, &mut cfg.font)?;
    }
    if let Ok(v) = t.get::<LuaTable>("colors") {
        parse_colors(&v, &mut cfg.colors)?;
    }
    Ok(())
}

fn parse_border(t: &LuaTable, cfg: &mut BorderConfig) -> Result<()> {
    if let Ok(v) = t.get::<f32>("width") { cfg.width = v; }
    if let Ok(v) = t.get::<f32>("radius") { cfg.radius = v; }
    if let Ok(v) = t.get::<String>("color") { cfg.color = Color::hex(&v); }
    Ok(())
}

fn parse_font(t: &LuaTable, cfg: &mut FontConfig) -> Result<()> {
    if let Ok(v) = t.get::<String>("family") { cfg.family = v; }
    if let Ok(v) = t.get::<f32>("size") { cfg.size = v; }
    if let Ok(v) = t.get::<f32>("summary_size") { cfg.summary_size = v; }
    Ok(())
}

fn parse_colors(t: &LuaTable, cfg: &mut ColorScheme) -> Result<()> {
    if let Ok(v) = t.get::<String>("summary") { cfg.summary = Color::hex(&v); }
    if let Ok(v) = t.get::<String>("body") { cfg.body = Color::hex(&v); }
    if let Ok(v) = t.get::<String>("app_name") { cfg.app_name = Color::hex(&v); }
    Ok(())
}

fn parse_animations(t: &LuaTable, cfg: &mut AnimationConfig) -> Result<()> {
    use notiser_types::animation::AnimationPreset;
    if let Ok(v) = t.get::<String>("preset") {
        cfg.preset = match v.as_str() {
            "none" => AnimationPreset::None,
            "standard" => AnimationPreset::Standard,
            "dynamic" => AnimationPreset::Dynamic,
            "custom" => AnimationPreset::Custom,
            _ => {
                warn!(value = v, "unknown animation preset, using standard");
                AnimationPreset::Standard
            }
        };
    }
    Ok(())
}

fn parse_urgency_overrides(
    t: &LuaTable,
    map: &mut HashMap<Urgency, UrgencyOverride>,
) -> Result<()> {
    for name in &["low", "normal", "critical"] {
        if let Ok(level_table) = t.get::<LuaTable>(*name) {
            let urgency = match *name {
                "low" => Urgency::Low,
                "normal" => Urgency::Normal,
                "critical" => Urgency::Critical,
                _ => unreachable!(),
            };
            let mut ov = UrgencyOverride {
                timeout: None,
                background: None,
                border_color: None,
                sound: None,
            };
            if let Ok(v) = level_table.get::<u32>("timeout") { ov.timeout = Some(v); }
            if let Ok(v) = level_table.get::<String>("background") { ov.background = Some(Color::hex(&v)); }
            if let Ok(v) = level_table.get::<String>("border_color") { ov.border_color = Some(Color::hex(&v)); }
            if let Ok(v) = level_table.get::<String>("sound") { ov.sound = Some(v); }
            map.insert(urgency, ov);
        }
    }
    Ok(())
}

fn parse_dnd(t: &LuaTable, cfg: &mut DndConfig) -> Result<()> {
    if let Ok(v) = t.get::<bool>("enabled") { cfg.enabled = v; }
    if let Ok(v) = t.get::<bool>("allow_critical") { cfg.allow_critical = v; }
    Ok(())
}

fn parse_history(t: &LuaTable, cfg: &mut HistoryConfig) -> Result<()> {
    if let Ok(v) = t.get::<bool>("enabled") { cfg.enabled = v; }
    if let Ok(v) = t.get::<u32>("max_entries") { cfg.max_entries = v; }
    if let Ok(v) = t.get::<u64>("ttl_seconds") { cfg.ttl_seconds = v; }
    if let Ok(v) = t.get::<bool>("store_transient") { cfg.store_transient = v; }
    Ok(())
}

fn parse_audio(t: &LuaTable, cfg: &mut AudioConfig) -> Result<()> {
    if let Ok(v) = t.get::<bool>("enabled") { cfg.enabled = v; }
    if let Ok(v) = t.get::<f32>("volume") { cfg.volume = v.clamp(0.0, 1.0); }
    if let Ok(v) = t.get::<u32>("cooldown_ms") { cfg.cooldown_ms = v; }
    Ok(())
}

fn parse_actions(t: &LuaTable, cfg: &mut ActionsConfig) -> Result<()> {
    if let Ok(v) = t.get::<String>("on_left_click") { cfg.on_left_click = parse_click_action(&v); }
    if let Ok(v) = t.get::<String>("on_right_click") { cfg.on_right_click = parse_click_action(&v); }
    if let Ok(v) = t.get::<String>("on_middle_click") { cfg.on_middle_click = parse_click_action(&v); }
    Ok(())
}

fn parse_click_action(s: &str) -> ClickAction {
    match s {
        "dismiss" => ClickAction::Dismiss,
        "dismiss_all" => ClickAction::DismissAll,
        "invoke_default" => ClickAction::InvokeDefault,
        "do_nothing" | "none" => ClickAction::DoNothing,
        other => ClickAction::InvokeAction(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = load_config_from_str("").unwrap();
        assert_eq!(config.general.default_timeout, 5000);
        assert_eq!(config.appearance.width, 360);
    }

    #[test]
    fn test_basic_setup() {
        let config = load_config_from_str(r##"
            notiser.setup({
                general = {
                    max_visible = 3,
                    default_timeout = 10000,
                },
                appearance = {
                    width = 400,
                    background = "#282828",
                    border = {
                        radius = 8.0,
                        width = 2.0,
                        color = "#504945",
                    },
                    colors = {
                        summary = "#ebdbb2",
                        body = "#d5c4a1",
                    },
                },
            })
        "##).unwrap();

        assert_eq!(config.general.max_visible, 3);
        assert_eq!(config.general.default_timeout, 10000);
        assert_eq!(config.appearance.width, 400);
        assert!(config.appearance.border.radius - 8.0 < f32::EPSILON);
    }

    #[test]
    fn test_urgency_overrides() {
        let config = load_config_from_str(r##"
            notiser.setup({
                urgency = {
                    critical = {
                        timeout = 0,
                        background = "#cc241d",
                        border_color = "#fb4934",
                    },
                },
            })
        "##).unwrap();

        let critical = config.urgency.get(&Urgency::Critical).unwrap();
        assert_eq!(critical.timeout, Some(0));
    }

    #[test]
    fn test_sandboxing() {
        let result = load_config_from_str(r#"
            os.execute("echo pwned")
        "#);
        assert!(result.is_err());
    }
}
