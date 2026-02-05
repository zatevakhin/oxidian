use std::time::Duration;

#[derive(Debug, Clone)]
pub struct VaultConfig {
    /// Directory names to ignore anywhere in the path.
    pub ignore_dirs: Vec<String>,
    /// File extensions (without dot) that are considered notes.
    pub note_extensions: Vec<String>,
    /// File extensions (without dot) that are considered attachments.
    pub attachment_extensions: Vec<String>,
    /// Debounce window for filesystem events.
    pub watch_debounce: Duration,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            ignore_dirs: vec![
                ".obsidian".into(),
                ".git".into(),
                ".trash".into(),
                "node_modules".into(),
                "target".into(),
            ],
            note_extensions: vec!["md".into(), "canvas".into()],
            attachment_extensions: vec![
                "png".into(),
                "jpg".into(),
                "jpeg".into(),
                "gif".into(),
                "webp".into(),
                "svg".into(),
                "pdf".into(),
            ],
            watch_debounce: Duration::from_millis(400),
        }
    }
}
