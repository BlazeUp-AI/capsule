//! Optional provider API key pool for Claude Code routing and Observal insights.
//!
//! Claude Code can use Anthropic keys directly. OpenRouter keys are used by
//! pointing Claude Code at OpenRouter's Anthropic-compatible endpoint.

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

        push_anthropic_keys(&mut keys, "ANTHROPIC_API_KEY");
        push_anthropic_keys(&mut keys, "CAPSULE_ANTHROPIC_API_KEY");
        push_anthropic_keys(&mut keys, "CAPSULE_ANTHROPIC_API_KEYS");
        push_anthropic_keys(&mut keys, "CAPSULE_FREE_KEYS_ANTHROPIC");
        push_openrouter_keys(&mut keys, "CAPSULE_FREE_KEYS_OPENROUTER");
        push_openrouter_keys(&mut keys, "OPENROUTER_API_KEY");
        push_openrouter_keys(&mut keys, "CAPSULE_OPENROUTER_API_KEYS");

        info!(
            total_keys = keys.len(),
            anthropic = keys.iter().filter(|k| k.provider == "anthropic").count(),
            openrouter = keys.iter().filter(|k| k.provider == "openrouter").count(),
            "Provider key pool initialized"
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

fn push_openrouter_keys(keys: &mut Vec<ProviderKey>, env_name: &str) {
    if let Ok(value) = std::env::var(env_name) {
        for key in value
            .split(',')
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            keys.push(ProviderKey {
                provider: "openrouter",
                env_var: "OPENROUTER_API_KEY",
                key: key.to_string(),
            });
        }
    }
}

fn push_anthropic_keys(keys: &mut Vec<ProviderKey>, env_name: &str) {
    if let Ok(value) = std::env::var(env_name) {
        for key in value
            .split(',')
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            keys.push(ProviderKey {
                provider: "anthropic",
                env_var: "ANTHROPIC_API_KEY",
                key: key.to_string(),
            });
        }
    }
}
