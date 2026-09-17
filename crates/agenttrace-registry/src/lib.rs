use std::{collections::BTreeMap, sync::Arc};

use agenttrace_adapter_aider::AiderAdapter;
use agenttrace_adapter_api::HarnessAdapter;
use agenttrace_adapter_claude_code::ClaudeCodeAdapter;
use agenttrace_adapter_cline::ClineAdapter;
use agenttrace_adapter_codex::CodexAdapter;
use agenttrace_adapter_continue::ContinueAdapter;
use agenttrace_adapter_gemini::GeminiAdapter;
use agenttrace_adapter_goose::GooseAdapter;
use agenttrace_adapter_opencode::OpenCodeAdapter;
use agenttrace_adapter_pi::PiAdapter;
use agenttrace_adapter_roo_code::RooCodeAdapter;
use agenttrace_protocol::HarnessId;

#[derive(Clone)]
pub struct AdapterRegistry {
    adapters: BTreeMap<HarnessId, Arc<dyn HarnessAdapter>>,
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        let adapters: [Arc<dyn HarnessAdapter>; 10] = [
            Arc::new(CodexAdapter::default()),
            Arc::new(ClaudeCodeAdapter::default()),
            Arc::new(OpenCodeAdapter::default()),
            Arc::new(PiAdapter::default()),
            Arc::new(GeminiAdapter::default()),
            Arc::new(AiderAdapter::default()),
            Arc::new(GooseAdapter::default()),
            Arc::new(ClineAdapter::default()),
            Arc::new(RooCodeAdapter),
            Arc::new(ContinueAdapter::default()),
        ];
        Self {
            adapters: adapters
                .into_iter()
                .map(|adapter| (adapter.id(), adapter))
                .collect(),
        }
    }
}

impl AdapterRegistry {
    pub fn get(&self, harness: HarnessId) -> Option<Arc<dyn HarnessAdapter>> {
        self.adapters.get(&harness).cloned()
    }

    pub fn iter(&self) -> impl Iterator<Item = (HarnessId, Arc<dyn HarnessAdapter>)> + '_ {
        self.adapters
            .iter()
            .map(|(harness, adapter)| (*harness, Arc::clone(adapter)))
    }

    pub fn harnesses(&self) -> impl Iterator<Item = HarnessId> + '_ {
        self.adapters.keys().copied()
    }

    pub fn parse(input: &str) -> Option<HarnessId> {
        match input.trim().to_ascii_lowercase().as_str() {
            "codex" | "openai-codex" => Some(HarnessId::Codex),
            "claude" | "claude-code" => Some(HarnessId::ClaudeCode),
            "opencode" | "open-code" => Some(HarnessId::Opencode),
            "pi" => Some(HarnessId::Pi),
            "gemini" | "gemini-cli" => Some(HarnessId::Gemini),
            "aider" => Some(HarnessId::Aider),
            "goose" => Some(HarnessId::Goose),
            "cline" => Some(HarnessId::Cline),
            "roo" | "roo-code" => Some(HarnessId::RooCode),
            "continue" | "continue-cli" => Some(HarnessId::Continue),
            _ => None,
        }
    }

    pub fn canonical_name(harness: HarnessId) -> &'static str {
        match harness {
            HarnessId::Codex => "codex",
            HarnessId::ClaudeCode => "claude-code",
            HarnessId::Opencode => "opencode",
            HarnessId::Pi => "pi",
            HarnessId::Gemini => "gemini",
            HarnessId::Aider => "aider",
            HarnessId::Goose => "goose",
            HarnessId::Cline => "cline",
            HarnessId::RooCode => "roo-code",
            HarnessId::Continue => "continue",
            HarnessId::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_all_initial_harnesses() {
        let registry = AdapterRegistry::default();
        assert_eq!(registry.harnesses().count(), 10);
        assert!(registry.get(HarnessId::Codex).is_some());
        assert!(registry.get(HarnessId::Continue).is_some());
    }

    #[test]
    fn aliases_resolve_without_guessing_unknown_names() {
        assert_eq!(AdapterRegistry::parse("gemini-cli"), Some(HarnessId::Gemini));
        assert_eq!(AdapterRegistry::parse("roo"), Some(HarnessId::RooCode));
        assert_eq!(AdapterRegistry::parse("made-up-agent"), None);
    }
}
