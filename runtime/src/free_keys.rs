//! Free-tier API key pool with round-robin rotation and fallback priority.
//!
//! Priority: Gemini → Groq → OpenRouter
//! Keys are configured via comma-separated env vars.

use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::info;

#[derive(Debug, Clone)]
pub struct ProviderKey {
    pub provider: &'static str,
    pub env_var: &'static str,
    pub key: String,
}

pub struct FreeKeyPool {
    keys: Vec<ProviderKey>,
    index: AtomicUsize,
}

impl FreeKeyPool {
    pub fn from_env() -> Self {
        let mut keys = Vec::new();

        if let Ok(val) = std::env::var("CAPSULE_FREE_KEYS_GEMINI") {
            for k in val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                keys.push(ProviderKey {
                    provider: "gemini",
                    env_var: "GOOGLE_API_KEY",
                    key: k.to_string(),
                });
            }
        }

        if let Ok(val) = std::env::var("CAPSULE_FREE_KEYS_GROQ") {
            for k in val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                keys.push(ProviderKey {
                    provider: "groq",
                    env_var: "GROQ_API_KEY",
                    key: k.to_string(),
                });
            }
        }

        if let Ok(val) = std::env::var("CAPSULE_FREE_KEYS_OPENROUTER") {
            for k in val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                keys.push(ProviderKey {
                    provider: "openrouter",
                    env_var: "OPENROUTER_API_KEY",
                    key: k.to_string(),
                });
            }
        }

        info!(
            total_keys = keys.len(),
            gemini = keys.iter().filter(|k| k.provider == "gemini").count(),
            groq = keys.iter().filter(|k| k.provider == "groq").count(),
            openrouter = keys.iter().filter(|k| k.provider == "openrouter").count(),
            "Free key pool initialized"
        );

        Self {
            keys,
            index: AtomicUsize::new(0),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Get the next key via round-robin. Returns None if pool is empty.
    pub fn next_key(&self) -> Option<&ProviderKey> {
        if self.keys.is_empty() {
            return None;
        }
        let idx = self.index.fetch_add(1, Ordering::Relaxed) % self.keys.len();
        Some(&self.keys[idx])
    }
}
