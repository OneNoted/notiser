use notiser_types::layout::{FlexAlign, FlexContainer, FlexDirection, LayoutNode};

#[derive(Debug, Clone)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct ResolvedLayout {
    pub node: ResolvedNode,
}

#[derive(Debug, Clone)]
pub enum ResolvedNode {
    Container {
        rect: LayoutRect,
        children: Vec<ResolvedNode>,
    },
    Text {
        rect: LayoutRect,
        kind: notiser_types::layout::TextKind,
    },
    Image {
        rect: LayoutRect,
        kind: notiser_types::layout::ImageKind,
    },
    Progress {
        rect: LayoutRect,
    },
    Actions {
        rect: LayoutRect,
    },
    Spacer {
        rect: LayoutRect,
    },
}
