import { invoke } from "@tauri-apps/api/core";

/** Claude Desktop 某账号/组织下的会话分组。 */
export interface DesktopSessionAccount {
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
  fromAccount: string;
  fromOrg?: string | null;
  /** 缺省为当前登录账号。 */
  toAccount?: string | null;
  toOrg?: string | null;
  dryRun: boolean;
}

export const desktopSessionsApi = {
  async listAccounts(): Promise<DesktopSessionAccount[]> {
    return await invoke("list_desktop_session_accounts");
  },

  async migrate(options: MigrateOptions): Promise<MigrateReport> {
    const {
      fromAccount,
      fromOrg = null,
      toAccount = null,
      toOrg = null,
      dryRun,
    } = options;
    return await invoke("migrate_desktop_sessions", {
      fromAccount,
      fromOrg,
      toAccount,
      toOrg,
      dryRun,
    });
  },
};
