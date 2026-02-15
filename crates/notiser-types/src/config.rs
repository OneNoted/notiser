use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::animation::AnimationPreset;
use crate::layout::LayoutNode;
use crate::notification::Urgency;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,
    pub display: DisplayConfig,
    pub appearance: AppearanceConfig,
    pub animations: AnimationConfig,
    pub urgency: HashMap<Urgency, UrgencyOverride>,
    pub apps: Vec<AppRule>,
    pub grouping: GroupingConfig,
    pub dnd: DndConfig,
    pub history: HistoryConfig,
    pub audio: AudioConfig,
    pub actions: ActionsConfig,
    pub layout: Option<LayoutNode>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            display: DisplayConfig::default(),
            appearance: AppearanceConfig::default(),
            animations: AnimationConfig::default(),
            urgency: HashMap::new(),
            apps: Vec::new(),
            grouping: GroupingConfig::default(),
            dnd: DndConfig::default(),
            history: HistoryConfig::default(),
            audio: AudioConfig::default(),
            actions: ActionsConfig::default(),
            layout: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub max_visible: u32,
    pub default_timeout: u32,
    pub sort_order: SortOrder,
    pub idle_threshold: u32,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            max_visible: 5,
            default_timeout: 5000,
            sort_order: SortOrder::TimeAscending,
            idle_threshold: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SortOrder {
    TimeAscending,
    TimeDescending,
    UrgencyDescending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub show_on: ShowOn,
    pub anchor: Anchor,
    pub margin: Margins,
    pub gap: u32,
    pub layer: Layer,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            show_on: ShowOn::All,
            anchor: Anchor::TopRight,
            margin: Margins {
                top: 10,
                right: 10,
                bottom: 10,
                left: 10,
            },
            gap: 8,
            layer: Layer::Overlay,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShowOn {
    All,
    Focused,
    FollowMouse,
    Output(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    CenterLeft,
    CenterRight,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Margins {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Layer {
    Background,
    Bottom,
    Top,
    Overlay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    pub width: u32,
    pub opacity: f32,
    pub background: Color,
    pub border: BorderConfig,
    pub padding: Margins,
    pub font: FontConfig,
    pub colors: ColorScheme,
    pub icon_size: u32,
    pub icon_theme: Option<String>,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            width: 360,
            opacity: 1.0,
            background: Color::hex("#1e1e2e"),
            border: BorderConfig::default(),
            padding: Margins {
                top: 12,
                right: 16,
                bottom: 12,
                left: 16,
            },
            font: FontConfig::default(),
            colors: ColorScheme::default(),
            icon_size: 48,
            icon_theme: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BorderConfig {
    pub width: f32,
    pub radius: f32,
    pub color: Color,
}

impl Default for BorderConfig {
    fn default() -> Self {
        Self {
            width: 1.0,
            radius: 12.0,
            color: Color::hex("#313244"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub summary_size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "sans-serif".into(),
            size: 14.0,
            summary_size: 15.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorScheme {
    pub summary: Color,
    pub body: Color,
    pub app_name: Color,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            summary: Color::hex("#cdd6f4"),
            body: Color::hex("#bac2de"),
            app_name: Color::hex("#a6adc8"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    #[must_use]
    pub fn hex(s: &str) -> Self {
        let s = s.strip_prefix('#').unwrap_or(s);
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(0);
        let a = if s.len() >= 8 {
            u8::from_str_radix(&s[6..8], 16).unwrap_or(255)
        } else {
            255
        };
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    #[must_use]
    pub fn to_array(&self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationConfig {
    pub preset: AnimationPreset,
    pub bezier_curves: HashMap<String, [f64; 4]>,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            preset: AnimationPreset::Standard,
            bezier_curves: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrgencyOverride {
    pub timeout: Option<u32>,
    pub background: Option<Color>,
    pub border_color: Option<Color>,
    pub sound: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRule {
    pub match_app_name: Option<String>,
    pub match_app_id: Option<String>,
    pub timeout: Option<u32>,
    pub urgency: Option<Urgency>,
    pub background: Option<Color>,
    pub group: Option<bool>,
    pub sound: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupingConfig {
    pub enabled: bool,
    pub by_app: bool,
    pub collapse_threshold: u32,
}

impl Default for GroupingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            by_app: true,
            collapse_threshold: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DndConfig {
    pub enabled: bool,
    pub allow_critical: bool,
}

impl Default for DndConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_critical: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    pub enabled: bool,
    pub max_entries: u32,
    pub ttl_seconds: u64,
    pub store_transient: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 100,
            ttl_seconds: 86400,
            store_transient: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub enabled: bool,
    pub volume: f32,
    pub cooldown_ms: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: 0.8,
            cooldown_ms: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionsConfig {
    pub on_left_click: ClickAction,
    pub on_right_click: ClickAction,
    pub on_middle_click: ClickAction,
}

impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            on_left_click: ClickAction::InvokeDefault,
            on_right_click: ClickAction::Dismiss,
            on_middle_click: ClickAction::DismissAll,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClickAction {
    Dismiss,
    DismissAll,
    InvokeDefault,
    InvokeAction(String),
    DoNothing,
}
