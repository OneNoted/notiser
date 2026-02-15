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

    // Register layout builder module
    register_layout_module(&lua)?;

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

    if let Ok(apps) = table.get::<LuaTable>("apps") {
        parse_app_rules(&apps, &mut config.apps)?;
    }

    if let Ok(actions) = table.get::<LuaTable>("actions") {
        parse_actions(&actions, &mut config.actions)?;
    }

    if let Ok(layout_table) = table.get::<LuaTable>("layout") {
        config.layout = Some(parse_layout_node(&layout_table)?);
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
    if let Ok(curves) = t.get::<LuaTable>("bezier_curves") {
        for pair in curves.pairs::<String, LuaTable>() {
            let (name, arr) = pair.map_err(lua_err)?;
            let x1 = arr.get::<f64>(1).unwrap_or(0.0);
            let y1 = arr.get::<f64>(2).unwrap_or(0.0);
            let x2 = arr.get::<f64>(3).unwrap_or(1.0);
            let y2 = arr.get::<f64>(4).unwrap_or(1.0);
            cfg.bezier_curves.insert(name, [x1, y1, x2, y2]);
        }
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

fn parse_app_rules(t: &LuaTable, rules: &mut Vec<AppRule>) -> Result<()> {
    for entry in t.sequence_values::<LuaTable>() {
        let rule_table = entry.map_err(lua_err)?;
        let mut rule = AppRule {
            match_app_name: rule_table.get::<String>("match_app_name").ok(),
            match_app_id: rule_table.get::<String>("match_app_id").ok(),
            timeout: rule_table.get::<u32>("timeout").ok(),
            urgency: None,
            background: rule_table.get::<String>("background").ok().map(|s| Color::hex(&s)),
            group: rule_table.get::<bool>("group").ok(),
            sound: rule_table.get::<String>("sound").ok(),
        };
        if let Ok(v) = rule_table.get::<String>("urgency") {
            rule.urgency = Some(match v.as_str() {
                "low" => Urgency::Low,
                "critical" => Urgency::Critical,
                _ => Urgency::Normal,
            });
        }
        rules.push(rule);
    }
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

// --- Layout DSL ---

/// Register the `notiser.layout` module with builder functions.
fn register_layout_module(lua: &Lua) -> Result<()> {
    // The layout module is exposed as a Lua table with functions that return
    // layout node tables. These tables are later parsed by parse_layout_node().
    //
    // Usage in init.lua:
    //   local l = require("notiser.layout")
    //   notiser.setup({ layout = l.flex({...}, { l.text({...}), ... }) })
    //
    // Since we don't have a real require() system, we expose it on the notiser table
    // and also install a package.preload entry.

    let layout_src = r#"
local M = {}

local function node(type_name, props, children)
    props = props or {}
    props._type = type_name
    if children then
        props._children = children
    end
    return props
end

function M.flex(props, children)
    return node("flex", props, children or {})
end

function M.text(props)
    return node("text", props)
end

function M.image(props)
    return node("image", props)
end

function M.progress(props)
    return node("progress", props)
end

function M.actions(props)
    return node("actions", props)
end

function M.spacer(props)
    return node("spacer", props)
end

function M.cond(predicate, child, fallback)
    return {
        _type = "cond",
        _predicate = predicate,
        _child = child,
        _fallback = fallback,
    }
end

-- Predicate constructors
function M.has(field)
    return { _pred = "has", field = field }
end

function M.has_hint(key)
    return { _pred = "has_hint", key = key }
end

function M.urgency(level)
    return { _pred = "urgency", level = level }
end

function M.app(pattern)
    return { _pred = "app", pattern = pattern }
end

function M.any(...)
    return { _pred = "any", predicates = {...} }
end

function M.all(...)
    return { _pred = "all", predicates = {...} }
end

function M.neg(pred)
    return { _pred = "not", inner = pred }
end

return M
"#;

    // Load the layout module and put it in package.preload and on notiser table
    let layout_module: LuaTable = lua.load(layout_src).set_name("layout").eval().map_err(lua_err)?;

    // Set notiser.layout = M
    let notiser: LuaTable = lua.globals().get("notiser").map_err(lua_err)?;
    notiser.set("layout", layout_module.clone()).map_err(lua_err)?;

    // Also make require("notiser.layout") work
    let package: LuaTable = lua.globals().get("package").map_err(lua_err)?;
    let preload: LuaTable = package.get("preload").map_err(lua_err)?;
    let layout_module_clone = layout_module;
    let loader = lua.create_function(move |_, ()| Ok(layout_module_clone.clone())).map_err(lua_err)?;
    preload.set("notiser.layout", loader).map_err(lua_err)?;

    Ok(())
}

/// Parse a Lua layout table (from the DSL) into a LayoutNode.
fn parse_layout_node(table: &LuaTable) -> Result<notiser_types::layout::LayoutNode> {
    use notiser_types::layout::*;

    let node_type: String = table.get("_type").map_err(lua_err)?;

    match node_type.as_str() {
        "flex" => {
            let direction = table
                .get::<String>("direction")
                .ok()
                .map(|s| match s.as_str() {
                    "row" => FlexDirection::Row,
                    _ => FlexDirection::Column,
                })
                .unwrap_or(FlexDirection::Column);

            let spacing = table.get::<f32>("spacing").unwrap_or(0.0);
            let align = table
                .get::<String>("align")
                .ok()
                .map(|s| match s.as_str() {
                    "center" => FlexAlign::Center,
                    "end" => FlexAlign::End,
                    "stretch" => FlexAlign::Stretch,
                    _ => FlexAlign::Start,
                })
                .unwrap_or(FlexAlign::Start);
            let flex = table.get::<f32>("flex").unwrap_or(1.0);

            let children_table: LuaTable = table.get("_children").map_err(lua_err)?;
            let mut children = Vec::new();
            for child in children_table.sequence_values::<LuaTable>() {
                let child = child.map_err(lua_err)?;
                children.push(parse_layout_node(&child)?);
            }

            Ok(LayoutNode::Flex(FlexContainer {
                direction,
                spacing,
                align,
                flex,
                children,
                padding: None,
            }))
        }
        "text" => {
            let kind = table
                .get::<String>("kind")
                .ok()
                .map(|s| match s.as_str() {
                    "summary" => TextKind::Summary,
                    "body" => TextKind::Body,
                    "app_name" => TextKind::AppName,
                    _ => TextKind::Body,
                })
                .unwrap_or(TextKind::Body);

            let max_lines = table.get::<u32>("max_lines").ok();
            let wrap = table.get::<bool>("wrap").unwrap_or(false);
            let markup = table.get::<bool>("markup").unwrap_or(false);
            let ellipsize = table
                .get::<String>("ellipsize")
                .ok()
                .map(|s| match s.as_str() {
                    "start" => Ellipsize::Start,
                    "middle" => Ellipsize::Middle,
                    "end" => Ellipsize::End,
                    _ => Ellipsize::None,
                })
                .unwrap_or(Ellipsize::None);

            let mut style = TextStyle::default();
            if let Ok(style_table) = table.get::<LuaTable>("style") {
                if let Ok(w) = style_table.get::<String>("weight") {
                    style.weight = Some(match w.as_str() {
                        "light" => FontWeight::Light,
                        "regular" => FontWeight::Regular,
                        "medium" => FontWeight::Medium,
                        "semibold" => FontWeight::Semibold,
                        "bold" => FontWeight::Bold,
                        _ => FontWeight::Regular,
                    });
                }
                if let Ok(s) = style_table.get::<f32>("size") {
                    style.size = Some(s);
                }
                if let Ok(c) = style_table.get::<String>("color") {
                    style.color = Some(Color::hex(&c));
                }
            }

            Ok(LayoutNode::Text(TextElement {
                kind,
                max_lines,
                wrap,
                ellipsize,
                markup,
                style,
            }))
        }
        "image" => {
            let kind = table
                .get::<String>("kind")
                .ok()
                .map(|s| match s.as_str() {
                    "app_icon" => ImageKind::AppIcon,
                    "image_data" => ImageKind::ImageData,
                    _ => ImageKind::AppIcon,
                })
                .unwrap_or(ImageKind::AppIcon);

            Ok(LayoutNode::Image(ImageElement {
                kind,
                width: table.get::<f32>("width").unwrap_or(48.0),
                height: table.get::<f32>("height").unwrap_or(48.0),
                rounding: table.get::<f32>("rounding").unwrap_or(0.0),
            }))
        }
        "progress" => {
            Ok(LayoutNode::Progress(ProgressElement {
                height: table.get::<f32>("height").unwrap_or(6.0),
                color: table
                    .get::<String>("color")
                    .ok()
                    .map(|s| Color::hex(&s))
                    .unwrap_or(Color::hex("#89b4fa")),
                background: table.get::<String>("background").ok().map(|s| Color::hex(&s)),
                border_radius: table.get::<f32>("border_radius").unwrap_or(3.0),
            }))
        }
        "actions" => {
            let layout = table
                .get::<String>("layout")
                .ok()
                .map(|s| match s.as_str() {
                    "column" => FlexDirection::Column,
                    _ => FlexDirection::Row,
                })
                .unwrap_or(FlexDirection::Row);
            let spacing = table.get::<f32>("spacing").unwrap_or(6.0);

            Ok(LayoutNode::Actions(ActionsElement { layout, spacing }))
        }
        "spacer" => {
            Ok(LayoutNode::Spacer(SpacerElement {
                flex: table.get::<f32>("flex").unwrap_or(1.0),
            }))
        }
        "cond" => {
            let pred_table: LuaTable = table.get("_predicate").map_err(lua_err)?;
            let predicate = parse_predicate(&pred_table)?;
            let child_table: LuaTable = table.get("_child").map_err(lua_err)?;
            let child = Box::new(parse_layout_node(&child_table)?);
            let fallback = table
                .get::<LuaTable>("_fallback")
                .ok()
                .map(|t| parse_layout_node(&t))
                .transpose()?
                .map(Box::new);

            Ok(LayoutNode::Conditional(ConditionalElement {
                predicate,
                child,
                fallback,
            }))
        }
        other => Err(anyhow::anyhow!("unknown layout node type: {other}")),
    }
}

fn parse_predicate(table: &LuaTable) -> Result<notiser_types::layout::Predicate> {
    use notiser_types::layout::Predicate;

    let pred_type: String = table.get("_pred").map_err(lua_err)?;

    match pred_type.as_str() {
        "has" => {
            let field: String = table.get("field").map_err(lua_err)?;
            Ok(Predicate::Has(field))
        }
        "has_hint" => {
            let key: String = table.get("key").map_err(lua_err)?;
            Ok(Predicate::HasHint(key))
        }
        "urgency" => {
            let level: String = table.get("level").map_err(lua_err)?;
            let urgency = match level.as_str() {
                "low" => notiser_types::notification::Urgency::Low,
                "critical" => notiser_types::notification::Urgency::Critical,
                _ => notiser_types::notification::Urgency::Normal,
            };
            Ok(Predicate::Urgency(urgency))
        }
        "app" => {
            let pattern: String = table.get("pattern").map_err(lua_err)?;
            Ok(Predicate::App(pattern))
        }
        "any" => {
            let preds: LuaTable = table.get("predicates").map_err(lua_err)?;
            let mut result = Vec::new();
            for p in preds.sequence_values::<LuaTable>() {
                result.push(parse_predicate(&p.map_err(lua_err)?)?);
            }
            Ok(Predicate::Any(result))
        }
        "all" => {
            let preds: LuaTable = table.get("predicates").map_err(lua_err)?;
            let mut result = Vec::new();
            for p in preds.sequence_values::<LuaTable>() {
                result.push(parse_predicate(&p.map_err(lua_err)?)?);
            }
            Ok(Predicate::All(result))
        }
        "not" => {
            let inner: LuaTable = table.get("inner").map_err(lua_err)?;
            Ok(Predicate::Not(Box::new(parse_predicate(&inner)?)))
        }
        other => Err(anyhow::anyhow!("unknown predicate type: {other}")),
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

    #[test]
    fn test_animation_config() {
        let config = load_config_from_str(r#"
            notiser.setup({
                animations = {
                    preset = "dynamic",
                    bezier_curves = {
                        my_curve = { 0.2, 0.9, 0.3, 1.1 },
                    },
                },
            })
        "#).unwrap();

        assert!(matches!(
            config.animations.preset,
            notiser_types::animation::AnimationPreset::Dynamic
        ));
        let curve = config.animations.bezier_curves.get("my_curve").unwrap();
        assert!((curve[0] - 0.2).abs() < f64::EPSILON);
        assert!((curve[3] - 1.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_app_rules() {
        let config = load_config_from_str(r##"
            notiser.setup({
                apps = {
                    { match_app_name = "Spotify", urgency = "low", timeout = 3000 },
                    { match_app_id = "org.mozilla.firefox", background = "#ff6611" },
                },
            })
        "##).unwrap();

        assert_eq!(config.apps.len(), 2);
        assert_eq!(config.apps[0].match_app_name.as_deref(), Some("Spotify"));
        assert_eq!(config.apps[0].timeout, Some(3000));
        assert_eq!(config.apps[1].match_app_id.as_deref(), Some("org.mozilla.firefox"));
        assert!(config.apps[1].background.is_some());
    }

    #[test]
    fn test_dynamic_island_config() {
        let home = std::env::var("HOME").unwrap();
        let source = std::fs::read_to_string(
            format!("{home}/.config/notiser/init.lua"),
        )
        .unwrap();
        let config = load_config_from_str(&source).unwrap();

        assert_eq!(config.general.max_visible, 3);
        assert_eq!(config.display.gap, 6);
        assert!(matches!(config.display.anchor, Anchor::TopCenter));
        assert!(config.appearance.border.radius - 22.0 < f32::EPSILON);
        assert!(matches!(
            config.animations.preset,
            notiser_types::animation::AnimationPreset::Dynamic,
        ));
        assert_eq!(config.apps.len(), 3);
        assert!(config.layout.is_some());

        // Verify layout structure: outer flex(row) with 2 children (cond + flex(col))
        match config.layout.unwrap() {
            notiser_types::layout::LayoutNode::Flex(f) => {
                assert!(matches!(
                    f.direction,
                    notiser_types::layout::FlexDirection::Row,
                ));
                assert_eq!(f.children.len(), 2);
            }
            _ => panic!("expected flex root"),
        }
    }

    #[test]
    fn test_layout_dsl() {
        let config = load_config_from_str(r#"
            local l = notiser.layout

            notiser.setup({
                layout = l.flex({ direction = "column", spacing = 4 }, {
                    l.text({ kind = "summary", style = { weight = "semibold" } }),
                    l.cond(
                        l.has("body"),
                        l.text({ kind = "body", wrap = true, max_lines = 3 })
                    ),
                }),
            })
        "#).unwrap();

        let layout = config.layout.unwrap();
        match layout {
            notiser_types::layout::LayoutNode::Flex(f) => {
                assert_eq!(f.children.len(), 2);
                assert!(f.spacing - 4.0 < f32::EPSILON);
            }
            _ => panic!("expected flex node"),
        }
    }
}
