import type { CredentialDiff } from "@/lib/api/settings";

export type ConfigurationState = "consistent" | "inconsistent";

export type RecoveryMode = "project_db_to_live" | "import_live_to_db";

export type CredentialField = "apiKey" | "baseUrl";

export interface ProviderSecurityStatus {
  providerId: string;
  appType: string;
  revision: number;
  credentialValid: boolean;
  conflicts: CredentialDiff[];
  configurationState: ConfigurationState;
}

export interface RecoveryResult {
  state: ConfigurationState;
  revision: number;
  liveFingerprintVerified: boolean;
  auditWritten: boolean;
}

/** Matches BE `#[serde(rename_all = "camelCase", tag = "kind")]` MutationOutcome. */
export type MutationOutcome =
  | {
      kind: "saved";
      revision: number;
      warnings: string[];
    }
  | {
      kind: "conflict";
      currentRevision: number;
      diff: CredentialDiff[];
    };

export interface ExplicitCredentialImport {
  appId: string;
  providerId: string;
  expectedRevision: number;
  fields: CredentialField[];
}
