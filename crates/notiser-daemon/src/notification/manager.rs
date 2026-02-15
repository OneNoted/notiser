use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use notiser_types::notification::Notification;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub struct NotificationManager {
    notifications: HashMap<u32, Notification>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: HashMap::new(),
        }
    }

    pub fn allocate_id() -> u32 {
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    }

    pub fn add(&mut self, notification: Notification) -> u32 {
        let id = notification.id;
        self.notifications.insert(id, notification);
        id
    }

    pub fn remove(&mut self, id: u32) -> Option<Notification> {
        self.notifications.remove(&id)
    }

    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.notifications.get(&id)
    }

    pub fn active_count(&self) -> usize {
        self.notifications.len()
    }

    pub fn active_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.notifications.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub fn iter(&self) -> impl Iterator<Item = &Notification> {
        self.notifications.values()
    }
}
