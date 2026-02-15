use std::collections::HashMap;

use notiser_types::config::ClickAction;
use tracing::debug;

// Linux button codes
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

/// Result of hit-testing a click position against notification cards.
pub enum HitResult {
    /// Click landed on a notification card.
    Notification(u32),
    /// Click didn't hit any card.
    None,
}

/// Hit-test: given a click position (x, y), determine which notification was clicked.
/// Uses per-notification sizes from the cache. Cards are center-aligned within the
/// surface, so x is checked against each card's centered rect.
pub fn hit_test(
    x: f64,
    y: f64,
    notification_ids: &[u32],
    card_sizes: &HashMap<u32, (f32, f32)>,
    fallback_size: (f32, f32),
    gap: f32,
    surface_padding: f32,
    h_pad: f32,
) -> HitResult {
    if notification_ids.is_empty() {
        return HitResult::None;
    }

    // Find the widest card to compute center offsets
    let max_card_width: f32 = notification_ids
        .iter()
        .map(|id| card_sizes.get(id).map(|s| s.0).unwrap_or(fallback_size.0))
        .fold(0.0f32, f32::max);

    let x = x as f32;
    let y = y as f32;
    let mut y_cursor = surface_padding;
    for &id in notification_ids {
        let (w, h) = card_sizes.get(&id).copied().unwrap_or(fallback_size);
        let center_offset = (max_card_width - w) / 2.0;
        let card_x = h_pad + center_offset;
        if x >= card_x && x < card_x + w && y >= y_cursor && y < y_cursor + h {
            return HitResult::Notification(id);
        }
        y_cursor += h + gap;
    }

    HitResult::None
}

/// Map a mouse button code to the corresponding click action from config.
pub fn action_for_button(
    button: u32,
    actions: &notiser_types::config::ActionsConfig,
) -> &ClickAction {
    match button {
        BTN_LEFT => &actions.on_left_click,
        BTN_RIGHT => &actions.on_right_click,
        BTN_MIDDLE => &actions.on_middle_click,
        _ => &ClickAction::DoNothing,
    }
}

/// Represents a pending click action to be executed by app.rs.
#[derive(Debug)]
pub enum InputAction {
    Dismiss(u32),
    DismissAll,
    InvokeDefault(u32),
    InvokeAction(u32, String),
}

/// Process a button press event, returning an action to execute.
pub fn process_click(
    button: u32,
    x: f64,
    y: f64,
    notification_ids: &[u32],
    card_sizes: &HashMap<u32, (f32, f32)>,
    fallback_size: (f32, f32),
    gap: f32,
    surface_padding: f32,
    h_pad: f32,
    actions_config: &notiser_types::config::ActionsConfig,
) -> Option<InputAction> {
    let hit = hit_test(x, y, notification_ids, card_sizes, fallback_size, gap, surface_padding, h_pad);
    let action = action_for_button(button, actions_config);

    match (action, hit) {
        (ClickAction::Dismiss, HitResult::Notification(id)) => {
            debug!(id, "dismiss click");
            Some(InputAction::Dismiss(id))
        }
        (ClickAction::DismissAll, _) => {
            debug!("dismiss all click");
            Some(InputAction::DismissAll)
        }
        (ClickAction::InvokeDefault, HitResult::Notification(id)) => {
            debug!(id, "invoke default click");
            Some(InputAction::InvokeDefault(id))
        }
        (ClickAction::InvokeAction(key), HitResult::Notification(id)) => {
            debug!(id, key, "invoke action click");
            Some(InputAction::InvokeAction(id, key.clone()))
        }
        (ClickAction::DoNothing, _) => None,
        (_, HitResult::None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_basic() {
        let ids = vec![1, 2, 3];
        let mut sizes = HashMap::new();
        sizes.insert(1, (200.0, 80.0));
        sizes.insert(2, (200.0, 80.0));
        sizes.insert(3, (200.0, 80.0));
        let gap = 8.0;
        let padding = 8.0;
        let h_pad = 0.0;

        // Hit first card (y=8..88, x=0..200)
        match hit_test(100.0, 20.0, &ids, &sizes, (200.0, 80.0), gap, padding, h_pad) {
            HitResult::Notification(id) => assert_eq!(id, 1),
            HitResult::None => panic!("expected hit"),
        }

        // Hit second card (y=96..176)
        match hit_test(100.0, 100.0, &ids, &sizes, (200.0, 80.0), gap, padding, h_pad) {
            HitResult::Notification(id) => assert_eq!(id, 2),
            HitResult::None => panic!("expected hit"),
        }

        // Miss (in gap between cards)
        match hit_test(100.0, 90.0, &ids, &sizes, (200.0, 80.0), gap, padding, h_pad) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss"),
        }
    }

    #[test]
    fn hit_test_variable_sizes() {
        let ids = vec![1, 2, 3];
        let mut sizes = HashMap::new();
        sizes.insert(1, (150.0, 50.0)); // y=8..58, narrower
        sizes.insert(2, (200.0, 100.0)); // y=66..166, widest
        sizes.insert(3, (180.0, 60.0)); // y=174..234
        let gap = 8.0;
        let padding = 8.0;
        let h_pad = 0.0;

        // Hit first (short/narrow) card, centered: x = (200-150)/2 = 25..175
        match hit_test(100.0, 30.0, &ids, &sizes, (200.0, 80.0), gap, padding, h_pad) {
            HitResult::Notification(id) => assert_eq!(id, 1),
            HitResult::None => panic!("expected hit"),
        }

        // Miss: x outside narrow card's centered rect
        match hit_test(10.0, 30.0, &ids, &sizes, (200.0, 80.0), gap, padding, h_pad) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss for x outside narrow card"),
        }

        // Hit second (tall/wide) card
        match hit_test(100.0, 120.0, &ids, &sizes, (200.0, 80.0), gap, padding, h_pad) {
            HitResult::Notification(id) => assert_eq!(id, 2),
            HitResult::None => panic!("expected hit"),
        }

        // In gap between first and second (y=58..66)
        match hit_test(100.0, 62.0, &ids, &sizes, (200.0, 80.0), gap, padding, h_pad) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss"),
        }
    }

    #[test]
    fn hit_test_empty() {
        match hit_test(50.0, 50.0, &[], &HashMap::new(), (200.0, 80.0), 8.0, 8.0, 0.0) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss"),
        }
    }
}
