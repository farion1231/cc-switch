import { invoke } from "@tauri-apps/api/core";

/**
 * Types mirroring `src-tauri/src/claude_desktop_data_migration.rs`.
 * Field names are camelCase to match the serde serialization of the commands.
 */

export interface AccountRootCandidate {
  path: string;
  metadataCount: number;
  scheduledCount: number;
  invalidJson: number;
  sizeBytes: number;
  folderReferences: string[];
}

export interface AppRootAudit {
  path: string;
  exists: boolean;
  deploymentMode?: string | null;
  sizeBytes: number;
  codeRoots: AccountRootCandidate[];
  coworkRoots: AccountRootCandidate[];
}

export interface ClaudeDesktopMigrationAudit {
  supported: boolean;
  sourceApp: AppRootAudit;
  targetApp: AppRootAudit;
  sharedCodeTranscriptRoot: string;
  sharedCodeTranscriptCount: number;
  sharedDocumentsRoot: string;
  sharedDocumentsExist: boolean;
}

export interface PathMapping {
  old: string;
  new: string;
}

/** Component resolution status reported by the backend. */
export type ComponentStatus =
  | "ready"
  | "source-empty"
  | "ambiguous-source"
  | "ambiguous-target"
  | "missing-target-seed";

export interface ComponentPlan {
  component: string;
  status: ComponentStatus | string;
  sourceRoot?: string | null;
  targetRoot?: string | null;
  candidates: string[];
  sourceMetadata: number;
  targetMetadata: number;
  newRecords: number;
  conflicts: number;
  invalidJson: number;
  missingSessionDirectories: number;
  scheduledRecords: number;
  missingSharedTranscripts: number;
  missingFolderPaths: string[];
  estimatedCopyBytes: number;
}

export interface ClaudeDesktopMigrationPlan {
  sourceApp: string;
  targetApp: string;
  code: ComponentPlan;
  cowork: ComponentPlan;
  pathMaps: PathMapping[];
  estimatedCopyBytes: number;
  estimatedBackupBytes: number;
  blockingIssues: string[];
  manualComponents: string[];
}

export interface MigrationRecord {
  component: string;
  id?: string;
  ids?: string[];
  targetMetadata?: string;
  targetSession?: string;
  target?: string;
  fingerprint?: string;
  reason?: string;
  error?: string;
}

export interface ClaudeDesktopMigrationApplyResult {
  backupPath: string;
  ledgerPath: string;
  installedCount: number;
  skippedCount: number;
  failedCount: number;
  failed: MigrationRecord[];
}

export interface MigrationVerifyCheck {
  name: string;
  ok: boolean;
  detail: string;
}

export interface ClaudeDesktopMigrationVerifyResult {
  passed: boolean;
  checks: MigrationVerifyCheck[];
}

export interface ClaudeDesktopMigrationRestoreResult {
  backupPath: string;
  removedCount: number;
  revertedCount: number;
  keptCount: number;
  skippedReason?: string;
  notes?: string[];
}

/** Root overrides + path mappings shared by plan/apply/verify. */
export interface MigrationRootsRequest {
  sourceApp?: string;
  targetApp?: string;
  sourceCodeRoot?: string;
  targetCodeRoot?: string;
  sourceCoworkRoot?: string;
  targetCoworkRoot?: string;
  pathMaps: PathMapping[];
}

/** Apply request: roots + component selection + explicit consent. */
export interface MigrationApplyRequest extends MigrationRootsRequest {
  components: string[];
  confirmed: boolean;
}

export const claudeDesktopMigrationApi = {
  /** Read-only inventory of the 1P/3P roots. */
  async audit(
    sourceApp?: string,
    targetApp?: string,
  ): Promise<ClaudeDesktopMigrationAudit> {
    return await invoke("audit_claude_desktop_data_migration", {
      sourceApp,
      targetApp,
    });
  },

  /** Build a read-only migration plan for the user to review. */
  async plan(
    request: MigrationRootsRequest,
  ): Promise<ClaudeDesktopMigrationPlan> {
    return await invoke("plan_claude_desktop_data_migration", { request });
  },

  /** Apply a reviewed plan (requires confirmed consent + quit Claude Desktop). */
  async apply(
    request: MigrationApplyRequest,
  ): Promise<ClaudeDesktopMigrationApplyResult> {
    return await invoke("apply_claude_desktop_data_migration", { request });
  },

  /** Read-only structural verification after a migration. */
  async verify(
    request: MigrationRootsRequest,
  ): Promise<ClaudeDesktopMigrationVerifyResult> {
    return await invoke("verify_claude_desktop_data_migration", { request });
  },

  /** Undo exactly what a migration installed (ledger-driven). */
  async restore(
    ledgerPath?: string,
  ): Promise<ClaudeDesktopMigrationRestoreResult> {
    return await invoke("restore_claude_desktop_data_migration", {
      ledgerPath,
    });
  },
};
