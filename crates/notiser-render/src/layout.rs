use notiser_types::layout::{FlexContainer, FlexDirection, LayoutNode, Predicate, TextKind};
use notiser_types::notification::Notification;

use crate::text::{TextEngine, measure_text_height, measure_text_width};

#[derive(Debug, Clone)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A positioned, concrete element ready for rendering.
#[derive(Debug)]
pub enum ResolvedElement {
    Text {
        rect: LayoutRect,
        kind: TextKind,
        content: String,
        font_size: f32,
        font_weight: Option<notiser_types::layout::FontWeight>,
        color: Option<notiser_types::config::Color>,
    },
    Image {
        rect: LayoutRect,
        kind: notiser_types::layout::ImageKind,
    },
    Progress {
        rect: LayoutRect,
        value: f32,
        color: notiser_types::config::Color,
        bg: Option<notiser_types::config::Color>,
        border_radius: f32,
    },
    Spacer,
}

/// Resolve a layout tree against a notification, producing positioned elements.
pub fn resolve_layout(
    layout: &LayoutNode,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    available: LayoutRect,
    text_engine: &mut TextEngine,
) -> Vec<ResolvedElement> {
    let mut elements = Vec::new();
    resolve_node(layout, notification, config, &available, &mut elements, text_engine);
    elements
}

fn resolve_node(
    node: &LayoutNode,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    available: &LayoutRect,
    out: &mut Vec<ResolvedElement>,
    text_engine: &mut TextEngine,
) {
    match node {
        LayoutNode::Flex(flex) => {
            resolve_flex(flex, notification, config, available, out, text_engine);
        }
        LayoutNode::Text(text) => {
            let content = match text.kind {
                TextKind::Summary => &notification.summary,
                TextKind::Body => &notification.body,
                TextKind::AppName => &notification.app_name,
            };
            let font_size = text.style.size.unwrap_or(match text.kind {
                TextKind::Summary => config.font.summary_size,
                TextKind::Body | TextKind::AppName => config.font.size,
            });
            out.push(ResolvedElement::Text {
                rect: available.clone(),
                kind: text.kind,
                content: content.clone(),
                font_size,
                font_weight: text.style.weight,
                color: text.style.color.clone(),
            });
        }
        LayoutNode::Image(img) => {
            out.push(ResolvedElement::Image {
                rect: LayoutRect {
                    x: available.x,
                    y: available.y,
                    width: img.width.min(available.width),
                    height: img.height.min(available.height),
                },
                kind: img.kind,
            });
        }
        LayoutNode::Progress(prog) => {
            let value = notification.hints.value.unwrap_or(0).clamp(0, 100) as f32 / 100.0;
            out.push(ResolvedElement::Progress {
                rect: LayoutRect {
                    x: available.x,
                    y: available.y,
                    width: available.width,
                    height: prog.height.min(available.height),
                },
                value,
                color: prog.color.clone(),
                bg: prog.background.clone(),
                border_radius: prog.border_radius,
            });
        }
        LayoutNode::Actions(_) => {
            // Actions rendering deferred to Phase 6
        }
        LayoutNode::Spacer(_) => {
            out.push(ResolvedElement::Spacer);
        }
        LayoutNode::Conditional(cond) => {
            if evaluate_predicate(&cond.predicate, notification) {
                resolve_node(&cond.child, notification, config, available, out, text_engine);
            } else if let Some(ref fallback) = cond.fallback {
                resolve_node(fallback, notification, config, available, out, text_engine);
            }
        }
    }
}

fn resolve_flex(
    flex: &FlexContainer,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    available: &LayoutRect,
    out: &mut Vec<ResolvedElement>,
    text_engine: &mut TextEngine,
) {
    let pad = flex.padding.unwrap_or([0.0; 4]);
    let inner_x = available.x + pad[3];
    let inner_y = available.y + pad[0];
    let inner_w = (available.width - pad[1] - pad[3]).max(0.0);
    let inner_h = (available.height - pad[0] - pad[2]).max(0.0);

    // Filter conditional children
    let active_children: Vec<&LayoutNode> = flex
        .children
        .iter()
        .filter(|child| match child {
            LayoutNode::Conditional(cond) => evaluate_predicate(&cond.predicate, notification),
            _ => true,
        })
        .collect();

    let n = active_children.len();
    if n == 0 {
        return;
    }

    let total_spacing = flex.spacing * (n as f32 - 1.0).max(0.0);
    let is_row = matches!(flex.direction, FlexDirection::Row);

    if is_row {
        // Row: distribute WIDTH using flex weights, cross = full height
        let main_available = inner_w - total_spacing;
        let cross_size = inner_h;

        let mut fixed_total: f32 = 0.0;
        let mut flex_total: f32 = 0.0;
        let mut child_sizes: Vec<ChildSize> = Vec::with_capacity(n);

        for child in &active_children {
            let (fixed, flex_weight) = measure_child(child, true);
            if flex_weight > 0.0 {
                flex_total += flex_weight;
                child_sizes.push(ChildSize::Flex(flex_weight));
            } else {
                let size = fixed.min(main_available);
                fixed_total += size;
                child_sizes.push(ChildSize::Fixed(size));
            }
        }

        let flex_space = (main_available - fixed_total).max(0.0);
        let mut offset = inner_x;

        for (i, child) in active_children.iter().enumerate() {
            let main_size = match child_sizes[i] {
                ChildSize::Fixed(s) => s,
                ChildSize::Flex(w) => {
                    if flex_total > 0.0 { flex_space * (w / flex_total) } else { 0.0 }
                }
            };

            let child_rect = LayoutRect {
                x: offset,
                y: inner_y,
                width: main_size,
                height: cross_size,
            };

            match child {
                LayoutNode::Conditional(cond) => {
                    resolve_node(&cond.child, notification, config, &child_rect, out, text_engine);
                }
                _ => {
                    resolve_node(child, notification, config, &child_rect, out, text_engine);
                }
            }

            offset += main_size + flex.spacing;
        }
    } else {
        // Column: use content-measured heights for children.
        // Measure each child's natural height, then distribute any remaining
        // space among truly-flex children (spacers).
        let cross_size = inner_w;

        let mut natural_heights: Vec<f32> = Vec::with_capacity(n);
        let mut flex_total: f32 = 0.0;
        let mut fixed_total: f32 = 0.0;

        for child in &active_children {
            let node = match child {
                LayoutNode::Conditional(cond) => &*cond.child,
                other => *other,
            };
            // Spacers are truly flexible — they absorb remaining space
            if matches!(node, LayoutNode::Spacer(s) if s.flex > 0.0) {
                if let LayoutNode::Spacer(s) = node {
                    flex_total += s.flex;
                }
                natural_heights.push(-1.0); // sentinel for flex child
            } else {
                let h = measure_node_height(node, notification, config, cross_size, text_engine);
                fixed_total += h;
                natural_heights.push(h);
            }
        }

        let flex_space = (inner_h - total_spacing - fixed_total).max(0.0);
        let mut offset = inner_y;

        for (i, child) in active_children.iter().enumerate() {
            let main_size = if natural_heights[i] < 0.0 {
                // Flex child (spacer): distribute remaining space
                let node = match child {
                    LayoutNode::Conditional(cond) => &*cond.child,
                    other => *other,
                };
                if let LayoutNode::Spacer(s) = node {
                    if flex_total > 0.0 { flex_space * (s.flex / flex_total) } else { 0.0 }
                } else {
                    0.0
                }
            } else {
                natural_heights[i]
            };

            let child_rect = LayoutRect {
                x: inner_x,
                y: offset,
                width: cross_size,
                height: main_size,
            };

            match child {
                LayoutNode::Conditional(cond) => {
                    resolve_node(&cond.child, notification, config, &child_rect, out, text_engine);
                }
                _ => {
                    resolve_node(child, notification, config, &child_rect, out, text_engine);
                }
            }

            offset += main_size + flex.spacing;
        }
    }
}

enum ChildSize {
    Fixed(f32),
    Flex(f32),
}

/// Measure a child's main-axis size. Returns (fixed_size, flex_weight).
fn measure_child(node: &LayoutNode, is_row: bool) -> (f32, f32) {
    match node {
        LayoutNode::Flex(f) => {
            if f.flex > 0.0 {
                (0.0, f.flex)
            } else {
                // Fixed flex containers - estimate from children
                (0.0, 1.0) // Default to flex=1 for containers
            }
        }
        LayoutNode::Text(_) => {
            // Text fills available space, treat as flex=1
            (0.0, 1.0)
        }
        LayoutNode::Image(img) => {
            let size = if is_row { img.width } else { img.height };
            (size, 0.0)
        }
        LayoutNode::Progress(p) => {
            if is_row {
                (0.0, 1.0) // Fills width
            } else {
                (p.height, 0.0) // Fixed height
            }
        }
        LayoutNode::Actions(_) => (0.0, 1.0),
        LayoutNode::Spacer(s) => (0.0, s.flex),
        LayoutNode::Conditional(cond) => measure_child(&cond.child, is_row),
    }
}

fn evaluate_predicate(pred: &Predicate, notification: &Notification) -> bool {
    match pred {
        Predicate::Has(field) => match field.as_str() {
            "body" => !notification.body.is_empty(),
            "icon" | "app_icon" => !notification.app_icon.is_empty(),
            "actions" => !notification.actions.is_empty(),
            "summary" => !notification.summary.is_empty(),
            "app_name" => !notification.app_name.is_empty(),
            _ => false,
        },
        Predicate::HasHint(key) => match key.as_str() {
            "value" => notification.hints.value.is_some(),
            "urgency" => notification.hints.urgency.is_some(),
            "category" => notification.hints.category.is_some(),
            "image-path" => notification.hints.image_path.is_some(),
            _ => notification.hints.extra.contains_key(key),
        },
        Predicate::Urgency(u) => notification.urgency() == *u,
        Predicate::App(pattern) => {
            notification.app_name.contains(pattern)
                || notification.hints.desktop_entry.as_ref().is_some_and(|e| e.contains(pattern))
        }
        Predicate::Any(preds) => preds.iter().any(|p| evaluate_predicate(p, notification)),
        Predicate::All(preds) => preds.iter().all(|p| evaluate_predicate(p, notification)),
        Predicate::Not(p) => !evaluate_predicate(p, notification),
    }
}

/// Measure the natural content width of a layout tree.
///
/// Text with `wrap == true && max_lines > 1` returns 0 (adapts to given width).
/// Text with `max_lines == Some(1)` or `wrap == false` measures unbounded width.
pub fn measure_layout_width(
    node: &LayoutNode,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    text_engine: &mut TextEngine,
) -> f32 {
    measure_node_width(node, notification, config, text_engine)
}

fn measure_node_width(
    node: &LayoutNode,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    text_engine: &mut TextEngine,
) -> f32 {
    match node {
        LayoutNode::Flex(flex) => measure_flex_width(flex, notification, config, text_engine),
        LayoutNode::Text(text) => {
            // Wrapping text with multiple lines doesn't drive width
            if text.wrap && text.max_lines != Some(1) {
                return 0.0;
            }
            let content = match text.kind {
                TextKind::Summary => &notification.summary,
                TextKind::Body => &notification.body,
                TextKind::AppName => &notification.app_name,
            };
            if content.is_empty() {
                return 0.0;
            }
            let font_size = text.style.size.unwrap_or(match text.kind {
                TextKind::Summary => config.font.summary_size,
                TextKind::Body | TextKind::AppName => config.font.size,
            });
            let buf = text_engine.create_buffer_unbounded(content, font_size);
            measure_text_width(&buf)
        }
        LayoutNode::Image(img) => img.width,
        LayoutNode::Progress(_) | LayoutNode::Actions(_) | LayoutNode::Spacer(_) => 0.0,
        LayoutNode::Conditional(cond) => {
            if evaluate_predicate(&cond.predicate, notification) {
                measure_node_width(&cond.child, notification, config, text_engine)
            } else if let Some(ref fallback) = cond.fallback {
                measure_node_width(fallback, notification, config, text_engine)
            } else {
                0.0
            }
        }
    }
}

fn measure_flex_width(
    flex: &FlexContainer,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    text_engine: &mut TextEngine,
) -> f32 {
    let pad = flex.padding.unwrap_or([0.0; 4]);
    let pad_lr = pad[1] + pad[3];

    let active_children: Vec<&LayoutNode> = flex
        .children
        .iter()
        .filter(|child| match child {
            LayoutNode::Conditional(cond) => evaluate_predicate(&cond.predicate, notification),
            _ => true,
        })
        .collect();

    let n = active_children.len();
    if n == 0 {
        return pad_lr;
    }

    let is_row = matches!(flex.direction, FlexDirection::Row);
    let total_spacing = flex.spacing * (n as f32 - 1.0).max(0.0);

    let child_widths: Vec<f32> = active_children
        .iter()
        .map(|child| {
            let node = match child {
                LayoutNode::Conditional(cond) => &*cond.child,
                other => *other,
            };
            measure_node_width(node, notification, config, text_engine)
        })
        .collect();

    if is_row {
        // Row: sum of children widths + spacing
        let sum: f32 = child_widths.iter().sum();
        pad_lr + sum + total_spacing
    } else {
        // Column: max of children widths
        let max = child_widths.iter().fold(0.0f32, |a, &b| a.max(b));
        pad_lr + max
    }
}

/// Measure the intrinsic height of a layout tree given an available width.
///
/// This mirrors `resolve_layout` but only computes vertical extent,
/// using `TextEngine` to measure wrapped text.
pub fn measure_layout_height(
    node: &LayoutNode,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    available_width: f32,
    text_engine: &mut TextEngine,
) -> f32 {
    measure_node_height(node, notification, config, available_width, text_engine)
}

fn measure_node_height(
    node: &LayoutNode,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    available_width: f32,
    text_engine: &mut TextEngine,
) -> f32 {
    match node {
        LayoutNode::Flex(flex) => {
            measure_flex_height(flex, notification, config, available_width, text_engine)
        }
        LayoutNode::Text(text) => {
            let content = match text.kind {
                TextKind::Summary => &notification.summary,
                TextKind::Body => &notification.body,
                TextKind::AppName => &notification.app_name,
            };
            if content.is_empty() {
                return 0.0;
            }
            let font_size = text.style.size.unwrap_or(match text.kind {
                TextKind::Summary => config.font.summary_size,
                TextKind::Body | TextKind::AppName => config.font.size,
            });
            let buf = text_engine.create_buffer(content, font_size, available_width);
            measure_text_height(&buf, text.max_lines)
        }
        LayoutNode::Image(img) => img.height,
        LayoutNode::Progress(prog) => prog.height,
        LayoutNode::Actions(_) => 0.0,
        LayoutNode::Spacer(_) => 0.0,
        LayoutNode::Conditional(cond) => {
            if evaluate_predicate(&cond.predicate, notification) {
                measure_node_height(&cond.child, notification, config, available_width, text_engine)
            } else if let Some(ref fallback) = cond.fallback {
                measure_node_height(fallback, notification, config, available_width, text_engine)
            } else {
                0.0
            }
        }
    }
}

fn measure_flex_height(
    flex: &FlexContainer,
    notification: &Notification,
    config: &notiser_types::config::AppearanceConfig,
    available_width: f32,
    text_engine: &mut TextEngine,
) -> f32 {
    let pad = flex.padding.unwrap_or([0.0; 4]);
    let inner_w = (available_width - pad[1] - pad[3]).max(0.0);

    // Filter active children (same logic as resolve_flex)
    let active_children: Vec<&LayoutNode> = flex
        .children
        .iter()
        .filter(|child| match child {
            LayoutNode::Conditional(cond) => evaluate_predicate(&cond.predicate, notification),
            _ => true,
        })
        .collect();

    let n = active_children.len();
    if n == 0 {
        return pad[0] + pad[2];
    }

    let is_row = matches!(flex.direction, FlexDirection::Row);
    let total_spacing = flex.spacing * (n as f32 - 1.0).max(0.0);

    // Distribute widths to children (mirrors resolve_flex logic)
    let main_available = if is_row { inner_w } else { inner_w } - if is_row { total_spacing } else { 0.0 };

    let mut fixed_total: f32 = 0.0;
    let mut flex_total: f32 = 0.0;
    let mut child_sizes: Vec<ChildSize> = Vec::with_capacity(n);

    for child in &active_children {
        let (fixed, flex_weight) = measure_child(child, is_row);
        if flex_weight > 0.0 {
            flex_total += flex_weight;
            child_sizes.push(ChildSize::Flex(flex_weight));
        } else {
            let size = fixed.min(main_available);
            fixed_total += size;
            child_sizes.push(ChildSize::Fixed(size));
        }
    }

    let flex_space = (main_available - fixed_total).max(0.0);

    if is_row {
        // Row: return max child height + padding
        let mut max_h: f32 = 0.0;
        for (i, child) in active_children.iter().enumerate() {
            let child_w = match child_sizes[i] {
                ChildSize::Fixed(s) => s,
                ChildSize::Flex(w) => {
                    if flex_total > 0.0 { flex_space * (w / flex_total) } else { 0.0 }
                }
            };
            let node = match child {
                LayoutNode::Conditional(cond) => &*cond.child,
                other => *other,
            };
            let h = measure_node_height(node, notification, config, child_w, text_engine);
            max_h = max_h.max(h);
        }
        pad[0] + max_h + pad[2]
    } else {
        // Column: sum child heights + spacing + padding
        let mut total_h: f32 = 0.0;
        let mut active_count = 0usize;
        for (i, child) in active_children.iter().enumerate() {
            let child_w = match child_sizes[i] {
                ChildSize::Fixed(_) => inner_w, // Column children get full width
                ChildSize::Flex(_) => inner_w,
            };
            let node = match child {
                LayoutNode::Conditional(cond) => &*cond.child,
                other => *other,
            };
            let h = measure_node_height(node, notification, config, child_w, text_engine);
            if h > 0.0 {
                total_h += h;
                active_count += 1;
            }
        }
        let spacing = if active_count > 1 {
            flex.spacing * (active_count as f32 - 1.0)
        } else {
            0.0
        };
        pad[0] + total_h + spacing + pad[2]
    }
}

/// Create the default layout tree when no custom layout is configured.
pub fn default_layout() -> LayoutNode {
    use notiser_types::layout::*;

    LayoutNode::Flex(FlexContainer {
        direction: FlexDirection::Column,
        spacing: 4.0,
        align: FlexAlign::Stretch,
        flex: 1.0,
        children: vec![
            LayoutNode::Text(TextElement {
                kind: TextKind::Summary,
                max_lines: Some(1),
                wrap: false,
                ellipsize: Ellipsize::End,
                markup: false,
                style: TextStyle {
                    weight: Some(FontWeight::Semibold),
                    size: None,
                    color: None,
                },
            }),
            LayoutNode::Conditional(ConditionalElement {
                predicate: Predicate::Has("body".into()),
                child: Box::new(LayoutNode::Text(TextElement {
                    kind: TextKind::Body,
                    max_lines: Some(3),
                    wrap: true,
                    ellipsize: Ellipsize::End,
                    markup: true,
                    style: TextStyle::default(),
                })),
                fallback: None,
            }),
        ],
        padding: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notiser_types::notification::{Notification, NotificationHints};
    use std::time::Instant;

    fn test_notification() -> Notification {
        Notification {
            id: 1,
            app_name: "test".into(),
            app_icon: String::new(),
            summary: "Hello".into(),
            body: "World".into(),
            actions: vec![],
            hints: NotificationHints::default(),
            expire_timeout: -1,
            created_at: Instant::now(),
            replaces_id: None,
        }
    }

    #[test]
    fn test_predicate_has_body() {
        let notification = test_notification();
        assert!(evaluate_predicate(&Predicate::Has("body".into()), &notification));
        assert!(evaluate_predicate(&Predicate::Has("summary".into()), &notification));
        assert!(evaluate_predicate(&Predicate::Has("app_name".into()), &notification));
        assert!(!evaluate_predicate(&Predicate::Has("app_icon".into()), &notification));
    }

    #[test]
    fn test_predicate_no_body() {
        let mut notification = test_notification();
        notification.body = String::new();
        assert!(!evaluate_predicate(&Predicate::Has("body".into()), &notification));
        assert!(evaluate_predicate(&Predicate::Has("summary".into()), &notification));
    }
}
