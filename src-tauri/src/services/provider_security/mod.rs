mod audit;
mod credentials;
mod mutation;
mod recovery;

use serde::{Deserialize, Serialize};

pub use audit::{
    prune_credential_audits, prune_snapshots, record_credential_audit, AUDIT_MAX_AGE_DAYS,
};
pub use credentials::{
    apply_selected_credentials, base_urls_equivalent, credential_fingerprint,
    extract_provider_credentials, mask_credential, normalize_base_url, CredentialFields,
};
pub(crate) use credentials::restore_selected_credentials;
pub use mutation::{MutationOutcome, ProviderMutationCoordinator, ProviderMutationRequest};
pub use recovery::{
    get_security_status, ConfigurationState, ProviderSecurityStatus, RecoveryMode, RecoveryResult,
};

pub const PROVIDER_REVISION_INITIAL: i64 = 1;
pub const ROLLBACK_MAX_VERSIONS: usize = 10;
pub const ROLLBACK_MAX_AGE_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialDiff {
    pub field: String,
    pub stored_masked: Option<String>,
    pub live_masked: Option<String>,
    pub stored_fingerprint: Option<String>,
    pub live_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    ProviderEdit,
    ExplicitLiveImport,
    CloudRestore,
    ExactRestore,
    Rollback,
    SystemProjection,
}

impl CredentialSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderEdit => "provider_edit",
            Self::ExplicitLiveImport => "explicit_live_import",
            Self::CloudRestore => "cloud_restore",
            Self::ExactRestore => "exact_restore",
            Self::Rollback => "rollback",
            Self::SystemProjection => "system_projection",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_source_uses_stable_snake_case_values() {
        assert_eq!(CredentialSource::ProviderEdit.as_str(), "provider_edit");
        assert_eq!(
            serde_json::to_string(&CredentialSource::ExplicitLiveImport).unwrap(),
            "\"explicit_live_import\""
        );
    }

    #[test]
    fn security_retention_constants_match_the_contract() {
        assert_eq!(PROVIDER_REVISION_INITIAL, 1);
        assert_eq!(ROLLBACK_MAX_VERSIONS, 10);
        assert_eq!(ROLLBACK_MAX_AGE_DAYS, 30);
        assert_eq!(AUDIT_MAX_AGE_DAYS, 90);
    }
}
