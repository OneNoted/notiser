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

/// Hit-test: given a click position (y), determine which notification was clicked.
/// Uses per-notification heights from the cache, with a fallback for unmeasured cards.
pub fn hit_test(
    y: f64,
    notification_ids: &[u32],
    card_heights: &HashMap<u32, f32>,
    fallback_height: f32,
    gap: f32,
    surface_padding: f32,
) -> HitResult {
    if notification_ids.is_empty() {
        return HitResult::None;
    }

    let y = y as f32;
    let mut y_cursor = surface_padding;
    for &id in notification_ids {
        let h = card_heights.get(&id).copied().unwrap_or(fallback_height);
        if y >= y_cursor && y < y_cursor + h {
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
    y: f64,
    notification_ids: &[u32],
    card_heights: &HashMap<u32, f32>,
    fallback_height: f32,
    gap: f32,
    surface_padding: f32,
    actions_config: &notiser_types::config::ActionsConfig,
) -> Option<InputAction> {
    let hit = hit_test(y, notification_ids, card_heights, fallback_height, gap, surface_padding);
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
        let mut heights = HashMap::new();
        heights.insert(1, 80.0);
        heights.insert(2, 80.0);
        heights.insert(3, 80.0);
        let gap = 8.0;
        let padding = 8.0;

        // Hit first card (y=8..88)
        match hit_test(20.0, &ids, &heights, 80.0, gap, padding) {
            HitResult::Notification(id) => assert_eq!(id, 1),
            HitResult::None => panic!("expected hit"),
        }

        // Hit second card (y=96..176)
        match hit_test(100.0, &ids, &heights, 80.0, gap, padding) {
            HitResult::Notification(id) => assert_eq!(id, 2),
            HitResult::None => panic!("expected hit"),
        }

        // Miss (in gap between cards)
        match hit_test(90.0, &ids, &heights, 80.0, gap, padding) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss"),
        }
    }

    #[test]
    fn hit_test_variable_heights() {
        let ids = vec![1, 2, 3];
        let mut heights = HashMap::new();
        heights.insert(1, 50.0); // y=8..58
        heights.insert(2, 100.0); // y=66..166
        heights.insert(3, 60.0); // y=174..234
        let gap = 8.0;
        let padding = 8.0;

        // Hit first (short) card
        match hit_test(30.0, &ids, &heights, 80.0, gap, padding) {
            HitResult::Notification(id) => assert_eq!(id, 1),
            HitResult::None => panic!("expected hit"),
        }

        // Hit second (tall) card
        match hit_test(120.0, &ids, &heights, 80.0, gap, padding) {
            HitResult::Notification(id) => assert_eq!(id, 2),
            HitResult::None => panic!("expected hit"),
        }

        // In gap between first and second (y=58..66)
        match hit_test(62.0, &ids, &heights, 80.0, gap, padding) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss"),
        }
    }

    #[test]
    fn hit_test_empty() {
        match hit_test(50.0, &[], &HashMap::new(), 80.0, 8.0, 8.0) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss"),
        }
    }
}
