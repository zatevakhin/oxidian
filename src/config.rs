use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_EMBEDDING_MODEL_URL: &str =
    "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx";
const DEFAULT_EMBEDDING_TOKENIZER_URL: &str =
    "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

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
    /// Minimum similarity score for note similarity health checks.
    pub similarity_min_score: f32,
    /// Maximum similar notes returned per source note.
    pub similarity_top_k: usize,
    /// Maximum notes to consider for similarity checks.
    pub similarity_max_notes: usize,
    /// Maximum token length for embedding inputs.
    pub embedding_max_length: usize,
    /// Base directory for cached embedding assets.
    pub embedding_cache_dir: PathBuf,
    /// URL to download the ONNX model from.
    pub embedding_model_url: String,
    /// URL to download the tokenizer JSON from.
    pub embedding_tokenizer_url: String,
    /// Vault schema TOML path (relative to vault root).
    pub schema_path: PathBuf,
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
            similarity_min_score: 0.75,
            similarity_top_k: 10,
            similarity_max_notes: 5000,
            embedding_max_length: 256,
            embedding_cache_dir: PathBuf::from(".obsidian/oxidian/embeddings"),
            embedding_model_url: DEFAULT_EMBEDDING_MODEL_URL.into(),
            embedding_tokenizer_url: DEFAULT_EMBEDDING_TOKENIZER_URL.into(),
            schema_path: PathBuf::from(".obsidian/oxidian/schema.toml"),
        }
    }
}
