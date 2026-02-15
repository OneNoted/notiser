use std::io::BufReader;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use tracing::{debug, warn};

use notiser_types::config::{AudioConfig, Config};
use notiser_types::notification::Notification;

pub struct AudioPlayer {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    volume: f32,
    cooldown: Duration,
    last_played: Option<Instant>,
}

impl AudioPlayer {
    pub fn new(config: &AudioConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        let (stream, handle) = match OutputStream::try_default() {
            Ok(pair) => pair,
            Err(e) => {
                warn!("failed to open audio output: {e}");
                return None;
            }
        };

        Some(Self {
            _stream: stream,
            handle,
            volume: config.volume,
            cooldown: Duration::from_millis(config.cooldown_ms as u64),
            last_played: None,
        })
    }

    /// Play a notification sound if appropriate.
    pub fn play_for_notification(&mut self, notification: &Notification, config: &Config) {
        // Respect suppress-sound hint
        if notification.hints.suppress_sound {
            return;
        }

        // Cooldown check
        if let Some(last) = self.last_played {
            if last.elapsed() < self.cooldown {
                return;
            }
        }

        // Determine sound file: hint > urgency override > default
        let sound_path = self.resolve_sound(notification, config);
        let Some(path) = sound_path else {
            return;
        };

        if !path.exists() {
            debug!(path = %path.display(), "sound file not found");
            return;
        }

        match self.play_file(&path) {
            Ok(()) => {
                self.last_played = Some(Instant::now());
                debug!(path = %path.display(), "played notification sound");
            }
            Err(e) => {
                warn!(path = %path.display(), "failed to play sound: {e}");
            }
        }
    }

    fn resolve_sound(&self, notification: &Notification, config: &Config) -> Option<PathBuf> {
        // 1. Hint sound-file takes priority
        if let Some(ref path) = notification.hints.sound_file {
            return Some(PathBuf::from(path));
        }

        // 2. Hint sound-name: look up in XDG sound theme
        if let Some(ref name) = notification.hints.sound_name {
            if let Some(path) = lookup_xdg_sound(name) {
                return Some(path);
            }
        }

        // 3. Per-urgency sound from config
        let urgency = notification.urgency();
        if let Some(ov) = config.urgency.get(&urgency) {
            if let Some(ref sound) = ov.sound {
                let p = PathBuf::from(sound);
                if p.exists() {
                    return Some(p);
                }
                // Try XDG sound lookup
                if let Some(path) = lookup_xdg_sound(sound) {
                    return Some(path);
                }
            }
        }

        None
    }

    fn play_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let file = std::fs::File::open(path)?;
        let reader = BufReader::new(file);
        let source = Decoder::new(reader)?;

        let sink = Sink::try_new(&self.handle)?;
        sink.set_volume(self.volume);
        sink.append(source);
        sink.detach();

        Ok(())
    }

    /// Update config (e.g. after hot reload).
    pub fn update_config(&mut self, config: &AudioConfig) {
        self.volume = config.volume;
        self.cooldown = Duration::from_millis(config.cooldown_ms as u64);
    }
}

/// Look up a sound name in the freedesktop sound theme directories.
fn lookup_xdg_sound(name: &str) -> Option<PathBuf> {
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/share:/usr/local/share".into());

    let extensions = ["oga", "ogg", "wav"];

    for dir in data_dirs.split(':') {
        for ext in &extensions {
            let path = PathBuf::from(dir)
                .join("sounds/freedesktop/stereo")
                .join(format!("{name}.{ext}"));
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}
