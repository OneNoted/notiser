use serde::{Deserialize, Serialize};

use crate::config::Color;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayoutNode {
    Flex(FlexContainer),
    Text(TextElement),
    Image(ImageElement),
    Progress(ProgressElement),
    Actions(ActionsElement),
    Spacer(SpacerElement),
    Conditional(ConditionalElement),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexContainer {
    pub direction: FlexDirection,
    pub spacing: f32,
    pub align: FlexAlign,
    pub flex: f32,
    pub children: Vec<LayoutNode>,
    pub padding: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FlexDirection {
    Row,
    Column,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FlexAlign {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextElement {
    pub kind: TextKind,
    pub max_lines: Option<u32>,
    pub wrap: bool,
    pub ellipsize: Ellipsize,
    pub markup: bool,
    pub style: TextStyle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TextKind {
    Summary,
    Body,
    AppName,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Ellipsize {
    None,
    Start,
    Middle,
    End,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextStyle {
    pub weight: Option<FontWeight>,
    pub size: Option<f32>,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FontWeight {
    Light,
    Regular,
    Medium,
    Semibold,
    Bold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageElement {
    pub kind: ImageKind,
    pub width: f32,
    pub height: f32,
    pub rounding: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ImageKind {
    AppIcon,
    ImageData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressElement {
    pub height: f32,
    pub color: Color,
    pub background: Option<Color>,
    pub border_radius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionsElement {
    pub layout: FlexDirection,
    pub spacing: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacerElement {
    pub flex: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionalElement {
    pub predicate: Predicate,
    pub child: Box<LayoutNode>,
    pub fallback: Option<Box<LayoutNode>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Predicate {
    Has(String),
    HasHint(String),
    Urgency(crate::notification::Urgency),
    App(String),
    Any(Vec<Predicate>),
    All(Vec<Predicate>),
    Not(Box<Predicate>),
}
