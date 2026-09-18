use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

pub const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Clone)]
pub struct RedactionRule {
    pub name: String,
    pub regex: Regex,
}

#[derive(Debug, Clone)]
pub struct RedactionConfig {
    pub rules: Vec<RedactionRule>,
    pub sensitive_json_keys: BTreeSet<String>,
    pub sensitive_path_fragments: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RedactionProfile {
    #[serde(default)]
    pub patterns: Vec<RedactionPatternSpec>,
    #[serde(default)]
    pub sensitive_json_keys: Vec<String>,
    #[serde(default)]
    pub sensitive_path_fragments: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionPatternSpec {
    pub name: String,
    pub regex: String,
}

#[derive(Debug, Error)]
pub enum RedactionProfileError {
    #[error("invalid redaction profile JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid regex for redaction rule `{name}`: {source}")]
    Regex {
        name: String,
        #[source]
        source: regex::Error,
    },
}

impl Default for RedactionConfig {
    fn default() -> Self {
        let patterns = [
            ("openai_key", r"\bsk-[A-Za-z0-9_-]{16,}\b"),
            ("anthropic_key", r"\bsk-ant-[A-Za-z0-9_-]{16,}\b"),
            (
                "github_token",
                r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}\b",
            ),
            ("aws_access_key", r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
            ("bearer", r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]{12,}"),
            (
                "private_key",
                r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
            ),
        ];
        Self {
            rules: patterns
                .into_iter()
                .map(|(name, pattern)| RedactionRule {
                    name: name.to_owned(),
                    regex: Regex::new(pattern).expect("built-in redaction regex must compile"),
                })
                .collect(),
            sensitive_json_keys: [
                "api_key",
                "apikey",
                "authorization",
                "cookie",
                "password",
                "secret",
                "token",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            sensitive_path_fragments: vec![
                ".ssh".into(),
                ".aws".into(),
                ".gnupg".into(),
                ".kube".into(),
                ".env".into(),
            ],
        }
    }
}

impl RedactionConfig {
    pub fn apply_profile(mut self, profile: RedactionProfile) -> Result<Self, RedactionProfileError> {
        for pattern in profile.patterns {
            let regex = Regex::new(&pattern.regex).map_err(|source| RedactionProfileError::Regex {
                name: pattern.name.clone(),
                source,
            })?;
            self.rules.push(RedactionRule {
                name: pattern.name,
                regex,
            });
        }
        self.sensitive_json_keys.extend(
            profile
                .sensitive_json_keys
                .into_iter()
                .map(|key| normalize_sensitive_key(&key)),
        );
        self.sensitive_path_fragments.extend(
            profile
                .sensitive_path_fragments
                .into_iter()
                .map(|fragment| fragment.replace('\\', "/").to_ascii_lowercase()),
        );
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct Redactor {
    config: RedactionConfig,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new(RedactionConfig::default())
    }
}

impl Redactor {
    pub fn new(config: RedactionConfig) -> Self {
        Self { config }
    }

    pub fn from_profile(profile: RedactionProfile) -> Result<Self, RedactionProfileError> {
        Ok(Self::new(RedactionConfig::default().apply_profile(profile)?))
    }

    pub fn from_profile_json(input: &str) -> Result<Self, RedactionProfileError> {
        let profile = serde_json::from_str(input)?;
        Self::from_profile(profile)
    }

    pub fn redact_str(&self, input: &str) -> String {
        self.config
            .rules
            .iter()
            .fold(input.to_owned(), |value, rule| {
                rule.regex.replace_all(&value, REDACTED).into_owned()
            })
    }

    pub fn redact_json(&self, value: &mut Value) {
        self.redact_json_inner(None, value);
    }

    fn redact_json_inner(&self, key: Option<&str>, value: &mut Value) {
        if key.is_some_and(|key| self.is_sensitive_key(key)) {
            *value = Value::String(REDACTED.to_owned());
            return;
        }
        match value {
            Value::String(text) => *text = self.redact_str(text),
            Value::Array(values) => {
                for value in values {
                    self.redact_json_inner(None, value);
                }
            }
            Value::Object(map) => {
                for (key, value) in map {
                    self.redact_json_inner(Some(key), value);
                }
            }
            _ => {}
        }
    }

    pub fn is_sensitive_path(&self, path: &str) -> bool {
        let normalized = path.replace('\\', "/").to_ascii_lowercase();
        self.config.sensitive_path_fragments.iter().any(|fragment| {
            let fragment = fragment.to_ascii_lowercase();
            normalized.split('/').any(|part| part == fragment)
        })
    }

    fn is_sensitive_key(&self, key: &str) -> bool {
        let normalized = normalize_sensitive_key(key);
        self.config.sensitive_json_keys.contains(&normalized)
            || normalized.ends_with("_token")
            || normalized.ends_with("_secret")
            || normalized.ends_with("_password")
            || normalized.ends_with("_api_key")
    }
}

fn normalize_sensitive_key(key: &str) -> String {
    key.to_ascii_lowercase().replace('-', "_")
}

#[derive(Debug, Clone, Default)]
pub struct EnvironmentPolicy {
    allowed_names: BTreeSet<String>,
}

impl EnvironmentPolicy {
    pub fn allow(mut self, name: impl Into<String>) -> Self {
        self.allowed_names.insert(name.into());
        self
    }

    pub fn filter<'a, I>(&self, environment: I, redactor: &Redactor) -> BTreeMap<String, String>
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        environment
            .into_iter()
            .filter(|(name, _)| self.allowed_names.contains(*name))
            .map(|(name, value)| (name.to_owned(), redactor.redact_str(value)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn redacts_keys_and_tokens_recursively() {
        let redactor = Redactor::default();
        let mut value = json!({
            "headers": {"authorization": "Bearer abcdefghijklmnopqrstuvwxyz"},
            "api_key": "not-even-pattern-dependent",
            "message": "token sk-abcdefghijklmnopqrstuvwxyz123456"
        });
        redactor.redact_json(&mut value);
        assert_eq!(value["headers"]["authorization"], REDACTED);
        assert_eq!(value["api_key"], REDACTED);
        assert!(value["message"].as_str().unwrap().contains(REDACTED));
    }

    #[test]
    fn environment_is_empty_by_default() {
        let redactor = Redactor::default();
        let env = [
            ("PATH", "/bin"),
            ("OPENAI_API_KEY", "sk-abcdefghijklmnopqrstuvwxyz"),
        ];
        assert!(
            EnvironmentPolicy::default()
                .filter(env, &redactor)
                .is_empty()
        );
    }

    #[test]
    fn recognizes_sensitive_paths_cross_platform() {
        let redactor = Redactor::default();
        assert!(redactor.is_sensitive_path("/home/me/.ssh/id_ed25519"));
        assert!(redactor.is_sensitive_path("C:\\Users\\me\\.aws\\credentials"));
        assert!(!redactor.is_sensitive_path("src/auth/token.rs"));
    }

    #[test]
    fn profile_adds_rules_keys_and_paths_without_disabling_defaults() {
        let redactor = Redactor::from_profile_json(
            r#"{
                "patterns": [{"name": "internal_ticket", "regex": "AT-[0-9]{6}"}],
                "sensitive_json_keys": ["customer_reference"],
                "sensitive_path_fragments": [".agenttrace-private"]
            }"#,
        )
        .unwrap();
        let mut value = json!({
            "ticket": "AT-123456",
            "customer_reference": "sensitive-customer",
            "builtin": "sk-abcdefghijklmnopqrstuvwxyz123456",
            "path": "/tmp/.agenttrace-private/context.json"
        });
        redact_paths(&mut value, &redactor);
        redactor.redact_json(&mut value);
        assert_eq!(value["ticket"], REDACTED);
        assert_eq!(value["customer_reference"], REDACTED);
        assert_eq!(value["builtin"], REDACTED);
        assert_eq!(value["path"], REDACTED);
    }

    #[test]
    fn invalid_profile_regex_is_rejected() {
        let error = Redactor::from_profile_json(
            r#"{"patterns":[{"name":"broken","regex":"("}]}"#,
        )
        .unwrap_err();
        assert!(matches!(error, RedactionProfileError::Regex { .. }));
    }

    fn redact_paths(value: &mut Value, redactor: &Redactor) {
        match value {
            Value::String(text) if redactor.is_sensitive_path(text) => {
                *text = REDACTED.to_owned();
            }
            Value::Array(values) => {
                for value in values {
                    redact_paths(value, redactor);
                }
            }
            Value::Object(map) => {
                for value in map.values_mut() {
                    redact_paths(value, redactor);
                }
            }
            _ => {}
        }
    }
}
