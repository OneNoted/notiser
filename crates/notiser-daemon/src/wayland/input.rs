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
/// Returns the notification ID if a card was hit.
pub fn hit_test(
    y: f64,
    notification_ids: &[u32],
    card_height: f32,
    gap: f32,
    surface_padding: f32,
) -> HitResult {
    if notification_ids.is_empty() {
        return HitResult::None;
    }

    let y = y as f32;
    for (i, &id) in notification_ids.iter().enumerate() {
        let card_y = surface_padding + i as f32 * (card_height + gap);
        if y >= card_y && y < card_y + card_height {
            return HitResult::Notification(id);
        }
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
    card_height: f32,
    gap: f32,
    surface_padding: f32,
    actions_config: &notiser_types::config::ActionsConfig,
) -> Option<InputAction> {
    let hit = hit_test(y, notification_ids, card_height, gap, surface_padding);
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
        let card_height = 80.0;
        let gap = 8.0;
        let padding = 8.0;

        // Hit first card (y=8..88)
        match hit_test(20.0, &ids, card_height, gap, padding) {
            HitResult::Notification(id) => assert_eq!(id, 1),
            HitResult::None => panic!("expected hit"),
        }

        // Hit second card (y=96..176)
        match hit_test(100.0, &ids, card_height, gap, padding) {
            HitResult::Notification(id) => assert_eq!(id, 2),
            HitResult::None => panic!("expected hit"),
        }

        // Miss (in gap between cards)
        match hit_test(90.0, &ids, card_height, gap, padding) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss"),
        }
    }

    #[test]
    fn hit_test_empty() {
        match hit_test(50.0, &[], 80.0, 8.0, 8.0) {
            HitResult::None => {}
            HitResult::Notification(_) => panic!("expected miss"),
        }
    }
}
