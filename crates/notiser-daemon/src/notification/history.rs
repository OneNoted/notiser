use std::collections::VecDeque;

use notiser_types::action::NotificationSummary;
use notiser_types::notification::Notification;

pub struct NotificationHistory {
    entries: VecDeque<NotificationSummary>,
    max_entries: usize,
}

impl NotificationHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
        }
    }

    pub fn push(&mut self, notification: &Notification) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }

        self.entries.push_back(NotificationSummary {
            id: notification.id,
            app_name: notification.app_name.clone(),
            summary: notification.summary.clone(),
            body: notification.body.clone(),
            urgency: format!("{:?}", notification.urgency()),
            timestamp: notification
                .created_at
                .elapsed()
                .as_secs(),
        });
    }

    pub fn recent(&self, limit: usize) -> Vec<NotificationSummary> {
        self.entries.iter().rev().take(limit).cloned().collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
