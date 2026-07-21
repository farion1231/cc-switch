import { invoke } from "@tauri-apps/api/core";

/** Claude Desktop 某账号/组织下的会话分组。 */
export interface DesktopSessionAccount {
  /** 数据根类型：`default`（.../Claude）或 `managed`（.../Claude-3p）。 */
  rootKind: string;
  accountUuid: string;
  orgUuid: string;
  sessionCount: number;
  isCurrent: boolean;
}

/** 一次迁移的结果报告（与后端 `MigrateReport` 对应）。 */
export interface MigrateReport {
  fromAccount: string;
  fromOrg: string;
  toAccount: string;
  toOrg: string;
  /** 来源目录的会话数。 */
  sourceCount: number;
  /** 迁移前目标目录的会话数。 */
  destCountBefore: number;
  /** 来源中目标尚不存在的会话数（dryRun 时即「将新增」数量）。 */
  pending: number;
  /** 实际复制的会话数（dryRun 时为 0）。 */
  copied: number;
  /** 迁移后目标目录的会话数。 */
  destCountAfter: number;
  dryRun: boolean;
}

export interface MigrateOptions {
  fromRoot: string;
  fromAccount: string;
  fromOrg?: string | null;
  toRoot: string;
  toAccount: string;
  toOrg?: string | null;
  dryRun: boolean;
}

export const desktopSessionsApi = {
  async listAccounts(): Promise<DesktopSessionAccount[]> {
    return await invoke("list_desktop_session_accounts");
  },

  async migrate(options: MigrateOptions): Promise<MigrateReport> {
    const {
      fromRoot,
      fromAccount,
      fromOrg = null,
      toRoot,
      toAccount,
      toOrg = null,
      dryRun,
    } = options;
    return await invoke("migrate_desktop_sessions", {
      fromRoot,
      fromAccount,
      fromOrg,
      toRoot,
      toAccount,
      toOrg,
      dryRun,
    });
  },
};
