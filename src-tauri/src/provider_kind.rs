//! The only place that decides which managed-account upstream a provider is.
//!
//! GitHub Copilot, Codex OAuth and xAI OAuth providers declare themselves in
//! `meta.providerType`. Copilot providers saved before that field existed carry
//! only their URL, so the URL fallback for that old data lives here and nowhere
//! else. `tests::identity_checks_stay_in_provider_kind` fails the build when a
//! raw `providerType` string or Copilot URL check shows up in another file.

use crate::provider::Provider;

const COPILOT_URL_MARKER: &str = "githubcopilot.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    GitHubCopilot,
    CodexOAuth,
    XaiOAuth,
}

impl ProviderKind {
    /// The kind written in `meta.providerType`, with no URL fallback.
    pub fn declared(provider: &Provider) -> Option<Self> {
        match provider.meta.as_ref()?.provider_type.as_deref()? {
            "github_copilot" => Some(Self::GitHubCopilot),
            "codex_oauth" => Some(Self::CodexOAuth),
            "xai_oauth" => Some(Self::XaiOAuth),
            _ => None,
        }
    }

    /// The provider's kind, counting old Copilot data that has no
    /// `providerType` but whose `env.ANTHROPIC_BASE_URL` points at Copilot.
    ///
    /// The URL wins over any declared kind, matching what
    /// `Provider::is_github_copilot` has always returned.
    pub fn of(provider: &Provider) -> Option<Self> {
        let declared = Self::declared(provider);
        let legacy_copilot_url = provider
            .settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str())
            .is_some_and(is_copilot_url);
        if declared == Some(Self::GitHubCopilot) || legacy_copilot_url {
            return Some(Self::GitHubCopilot);
        }
        declared
    }
}

/// Whether a resolved upstream URL is a GitHub Copilot endpoint. Call sites
/// that check a URL other than `env.ANTHROPIC_BASE_URL` use this next to
/// `ProviderKind::of` so the extra source stays visible.
pub fn is_copilot_url(url: &str) -> bool {
    url.contains(COPILOT_URL_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serde_json::json;

    fn provider(provider_type: Option<&str>, settings: serde_json::Value) -> Provider {
        let mut provider = Provider::with_id("p".into(), "P".into(), settings, None);
        provider.meta = provider_type.map(|kind| ProviderMeta {
            provider_type: Some(kind.to_string()),
            ..Default::default()
        });
        provider
    }

    fn env_url(url: &str) -> serde_json::Value {
        json!({ "env": { "ANTHROPIC_BASE_URL": url } })
    }

    #[test]
    fn declared_reads_only_provider_type() {
        let cases = [
            (Some("github_copilot"), Some(ProviderKind::GitHubCopilot)),
            (Some("codex_oauth"), Some(ProviderKind::CodexOAuth)),
            (Some("xai_oauth"), Some(ProviderKind::XaiOAuth)),
            (Some("newapi"), None),
            (None, None),
        ];
        for (provider_type, expected) in cases {
            let p = provider(provider_type, env_url("https://api.githubcopilot.com"));
            assert_eq!(ProviderKind::declared(&p), expected, "{provider_type:?}");
        }
    }

    #[test]
    fn of_counts_legacy_copilot_url_without_provider_type() {
        let legacy = provider(None, env_url("https://api.githubcopilot.com"));
        assert_eq!(ProviderKind::of(&legacy), Some(ProviderKind::GitHubCopilot));

        let other_type = provider(Some("newapi"), env_url("https://api.githubcopilot.com"));
        assert_eq!(
            ProviderKind::of(&other_type),
            Some(ProviderKind::GitHubCopilot)
        );

        let plain = provider(None, env_url("https://api.anthropic.com"));
        assert_eq!(ProviderKind::of(&plain), None);
    }

    #[test]
    fn of_reads_only_env_base_url_for_legacy_data() {
        // `baseUrl`, `base_url` and friends are not part of the legacy rule;
        // call sites that also trust them say so with `is_copilot_url`.
        let other_fields = provider(
            None,
            json!({
                "baseUrl": "https://api.githubcopilot.com",
                "base_url": "https://api.githubcopilot.com",
            }),
        );
        assert_eq!(ProviderKind::of(&other_fields), None);
    }

    #[test]
    fn of_lets_copilot_url_override_a_declared_oauth_kind() {
        let codex = provider(
            Some("codex_oauth"),
            env_url("https://api.githubcopilot.com"),
        );
        assert_eq!(ProviderKind::of(&codex), Some(ProviderKind::GitHubCopilot));
        assert_eq!(
            ProviderKind::declared(&codex),
            Some(ProviderKind::CodexOAuth)
        );

        let codex = provider(Some("codex_oauth"), env_url("https://chatgpt.com"));
        assert_eq!(ProviderKind::of(&codex), Some(ProviderKind::CodexOAuth));
    }

    /// Rust files allowed to spell the raw checks: this module only.
    const OWNER: &str = "provider_kind.rs";
    const FORBIDDEN: &[&str] = &[
        "contains(\"githubcopilot.com\")",
        "Some(\"github_copilot\")",
        "Some(\"codex_oauth\")",
        "Some(\"xai_oauth\")",
        "\"github_copilot\" | ",
    ];

    /// Production code before the first `#[cfg(test)]`; test fixtures may
    /// still build providers with raw `providerType` strings.
    fn production_part(source: &str) -> &str {
        source
            .find("#[cfg(test)]")
            .map_or(source, |end| &source[..end])
    }

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read src dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn identity_checks_stay_in_provider_kind() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        files.sort();

        let mut violations = Vec::new();
        for path in files {
            if path.file_name().is_some_and(|name| name == OWNER) {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read source");
            for (index, line) in production_part(&source).lines().enumerate() {
                if let Some(pattern) = FORBIDDEN.iter().find(|p| line.contains(*p)) {
                    let relative = path.strip_prefix(&src).unwrap_or(&path);
                    violations.push(format!(
                        "src/{}:{}: `{pattern}`",
                        relative.display(),
                        index + 1
                    ));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "provider identity checked outside src/provider_kind.rs:\n  {}\n\
             Use ProviderKind::of(provider) (or ProviderKind::declared when old \
             URL-only data must not count). For a resolved request URL, use \
             provider_kind::is_copilot_url next to ProviderKind::of.",
            violations.join("\n  ")
        );
    }
}
