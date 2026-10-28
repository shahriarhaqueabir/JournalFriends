use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub dims: Option<usize>,
}

pub struct ModelStore {
    models_dir: PathBuf,
}

/// Recommended (not required) chat GGUF. Bring your own instruct-tuned model;
/// this is the one the prompts and tool schemas were tuned against.
pub const RECOMMENDED_CHAT_MODEL: &str = "Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M";

/// Filename fragments that identify an *embedding* model. These are rejected
/// for the chat role because they cannot follow instructions or call tools.
const EMBEDDING_MODEL_HINTS: [&str; 8] = [
    "embed", "nomic", "bge-", "bge_", "gte-", "gte_", "e5-", "minilm",
];

/// True when the filename looks like an embedding model rather than an
/// instruct-tuned chat model.
pub fn is_embedding_model(filename: &str) -> bool {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_ascii_lowercase();
    EMBEDDING_MODEL_HINTS.iter().any(|hint| stem.contains(hint))
}

/// Any local GGUF is accepted for the chat role except a known embedding model.
/// "Instruct-tuned" cannot be read from a filename, so this is a permissive
/// guard: Qwythos is the recommended default, anything else is the user's call.
pub fn is_supported_chat_model(filename: &str) -> bool {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    !stem.is_empty() && !is_embedding_model(stem)
}

impl ModelStore {
    pub fn new(models_dir: PathBuf) -> Self {
        Self { models_dir }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.models_dir.join("chat"))?;
        std::fs::create_dir_all(self.models_dir.join("embed"))?;
        Ok(())
    }

    pub fn scan_chat(&self) -> Vec<ModelInfo> {
        self.scan_dir(&self.models_dir.join("chat"))
            .into_iter()
            .filter(|m| is_supported_chat_model(&m.filename))
            .collect()
    }

    pub fn scan_embed(&self) -> Vec<ModelInfo> {
        self.scan_dir(&self.models_dir.join("embed"))
    }

    fn scan_dir(&self, dir: &Path) -> Vec<ModelInfo> {
        let mut models = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("gguf") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        models.push(ModelInfo {
                            name: name.to_string(),
                            filename: path.file_name().unwrap().to_str().unwrap().to_string(),
                            size_bytes: size,
                            dims: None,
                        });
                    }
                }
            }
        }
        models
    }

    pub fn import_model(&self, source: &Path, target_type: &str) -> Result<ModelInfo, String> {
        if !source.exists() {
            return Err("Source file does not exist".into());
        }

        let ext = source.extension().and_then(|e| e.to_str());
        if ext != Some("gguf") {
            return Err("Not a GGUF file".into());
        }

        let filename = source.file_name().unwrap().to_str().unwrap().to_string();
        if target_type == "chat" && !is_supported_chat_model(&filename) {
            return Err(format!(
                "That file looks like an embedding model, so it cannot serve the chat role. Pick an instruct-tuned chat GGUF (recommended: {}.gguf) or put it in models/chat/.",
                RECOMMENDED_CHAT_MODEL
            ));
        }
        let dest_dir = self.models_dir.join(target_type);
        std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
        let dest = dest_dir.join(&filename);

        std::fs::copy(source, &dest).map_err(|e| format!("Copy failed: {}", e))?;

        let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        let name = source.file_stem().unwrap().to_str().unwrap().to_string();

        Ok(ModelInfo {
            name,
            filename,
            size_bytes: size,
            dims: None,
        })
    }

    pub fn get_chat_path(&self, filename: &str) -> PathBuf {
        self.models_dir.join("chat").join(filename)
    }

    pub fn get_embed_path(&self, filename: &str) -> PathBuf {
        self.models_dir.join("embed").join(filename)
    }

    /// Resolve a configured model name (GGUF filename or stem) to the absolute
    /// path of the actual file in models/chat/. Falls back to name-as-filename
    /// when nothing matches, so spawn fails loudly instead of silently.
    pub fn resolve_chat_path(&self, name: &str) -> PathBuf {
        let found = self
            .scan_chat()
            .into_iter()
            .find(|m| m.name == name || m.filename == name)
            .map(|m| m.filename)
            .unwrap_or_else(|| name.to_string());
        self.get_chat_path(&found)
    }

    /// Same as resolve_chat_path but for models/embed/.
    pub fn resolve_embed_path(&self, name: &str) -> PathBuf {
        let found = self
            .scan_embed()
            .into_iter()
            .find(|m| m.name == name || m.filename == name)
            .map(|m| m.filename)
            .unwrap_or_else(|| name.to_string());
        self.get_embed_path(&found)
    }

    pub fn remove_model(&self, target_type: &str, filename: &str) -> Result<(), String> {
        let path = self.models_dir.join(target_type).join(filename);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn chat_import_accepts_any_instruct_gguf_but_rejects_embedding_models() {
        let root = std::env::temp_dir().join(format!("model-role-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let qwythos = root.join("Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf");
        let nomic = root.join("nomic-embed-text-v2-moe.Q4_K_M.gguf");
        let other = root.join("Llama-3.1-8B-Instruct-Q4_K_M.gguf");
        write_file(&qwythos, b"test");
        write_file(&nomic, b"test");
        write_file(&other, b"test");
        let store = ModelStore::new(root.join("models"));

        assert!(store.import_model(&qwythos, "chat").is_ok());
        assert!(
            store.import_model(&other, "chat").is_ok(),
            "any instruct-tuned chat GGUF must be accepted, not only Qwythos"
        );
        assert!(
            store.import_model(&nomic, "chat").is_err(),
            "an embedding model must not be accepted as the chat role"
        );
        assert!(store.import_model(&nomic, "embed").is_ok());

        let mut chat_names: Vec<String> =
            store.scan_chat().into_iter().map(|m| m.filename).collect();
        chat_names.sort();
        assert_eq!(
            chat_names,
            vec![
                "Llama-3.1-8B-Instruct-Q4_K_M.gguf".to_string(),
                "Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf".to_string()
            ]
        );
        let embed_names: Vec<String> = store.scan_embed().into_iter().map(|m| m.filename).collect();
        assert_eq!(
            embed_names,
            vec!["nomic-embed-text-v2-moe.Q4_K_M.gguf".to_string()]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn supported_chat_model_accepts_any_gguf_except_embeddings() {
        assert!(is_supported_chat_model(
            "Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf"
        ));
        assert!(is_supported_chat_model("qwythos-9b-q8_0.gguf"));
        assert!(is_supported_chat_model("other-chat.gguf"));
        assert!(is_supported_chat_model(
            "Mistral-Nemo-Instruct-2407.Q5_K_M.gguf"
        ));
        assert!(!is_supported_chat_model(
            "nomic-embed-text-v2-moe.Q4_K_M.gguf"
        ));
        assert!(!is_supported_chat_model("bge-m3-Q8_0.gguf"));
        assert!(!is_supported_chat_model(""));
    }
}
