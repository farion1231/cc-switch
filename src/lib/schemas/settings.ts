import { z } from "zod";

const directorySchema = z
  .string()
  .trim()
  .min(1, "路径不能为空")
  .optional()
  .or(z.literal(""));

export const settingsSchema = z.object({
  // 设备级 UI 设置
  showInTray: z.boolean(),
  minimizeToTrayOnClose: z.boolean(),
  enableClaudePluginIntegration: z.boolean().optional(),
  skipClaudeOnboarding: z.boolean().optional(),
  launchOnStartup: z.boolean().optional(),
  enableLocalProxy: z.boolean().optional(),
  preserveCodexOfficialAuthOnSwitch: z.boolean().optional(),
  language: z.enum(["en", "zh", "zh-TW", "ja"]).optional(),

  // 设备级目录覆盖
  claudeConfigDir: directorySchema.nullable().optional(),
  codexConfigDir: directorySchema.nullable().optional(),
  geminiConfigDir: directorySchema.nullable().optional(),
  opencodeConfigDir: directorySchema.nullable().optional(),
  openclawConfigDir: directorySchema.nullable().optional(),

  // 当前供应商 ID（设备级）
  currentProviderClaude: z.string().optional(),
  currentProviderClaudeDesktop: z.string().optional(),
  currentProviderCodex: z.string().optional(),
  currentProviderGemini: z.string().optional(),

  // Skill 同步设置
  skillSyncMethod: z.enum(["auto", "symlink", "copy"]).optional(),
  skillStorageLocation: z.enum(["cc_switch", "unified"]).optional(),

  // Codex 用量刷新间隔（秒，默认 300）
  codexQuotaRefreshInterval: z.number().min(60).optional(),

  // 用量查询设置
  usageRefreshIntervalSecs: z.number().min(30).optional(),
  usageAutoRefresh: z.boolean().optional(),
  usageShowAllAccounts: z.boolean().optional(),

  // WebDAV v2 同步设置（通过专用命令保存，schema 仅用于读取）
  webdavSync: z
    .object({
      enabled: z.boolean().optional(),
      autoSync: z.boolean().optional(),
      baseUrl: z.string().trim().optional().or(z.literal("")),
      username: z.string().trim().optional().or(z.literal("")),
      password: z.string().optional(),
      remoteRoot: z.string().trim().optional().or(z.literal("")),
      profile: z.string().trim().optional().or(z.literal("")),
      status: z
        .object({
          lastSyncAt: z.number().nullable().optional(),
          lastError: z.string().nullable().optional(),
          lastErrorSource: z.string().nullable().optional(),
          lastRemoteEtag: z.string().nullable().optional(),
          lastLocalManifestHash: z.string().nullable().optional(),
          lastRemoteManifestHash: z.string().nullable().optional(),
        })
        .optional(),
    })
    .optional(),

  // 本机自动迁移状态（后端维护，前端保存设置时应透传）
  localMigrations: z
    .object({
      codexThirdPartyHistoryProviderBucketV1: z
        .object({
          completedAt: z.string(),
          targetProviderId: z.string(),
          sourceProviderIds: z.array(z.string()).optional(),
          migratedJsonlFiles: z.number().optional(),
          migratedStateRows: z.number().optional(),
        })
        .optional(),
    })
    .optional(),
});

export type SettingsFormData = z.infer<typeof settingsSchema>;
