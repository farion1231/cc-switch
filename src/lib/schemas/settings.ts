import { z } from "zod";

const directorySchema = z
  .string()
  .trim()
  .min(1, "路径不能为空")
  .optional()
  .or(z.literal(""));

const configDirectorySetSchema = z.object({
  id: z.string().min(1, "ID 不能为空"),
  name: z
    .string()
    .trim()
    .min(1, "名称不能为空")
    .max(60, "名称太长啦"),
  claudeConfigDir: directorySchema.nullable().optional(),
  codexConfigDir: directorySchema.nullable().optional(),
  geminiConfigDir: directorySchema.nullable().optional(),
  currentProviderClaude: z.string().trim().min(1).optional(),
  currentProviderCodex: z.string().trim().min(1).optional(),
  currentProviderGemini: z.string().trim().min(1).optional(),
});

export const settingsSchema = z.object({
  // 设备级 UI 设置
  showInTray: z.boolean(),
  minimizeToTrayOnClose: z.boolean(),
  enableClaudePluginIntegration: z.boolean().optional(),
  launchOnStartup: z.boolean().optional(),
  language: z.enum(["en", "zh", "ja"]).optional(),

  // 设备级目录覆盖
  claudeConfigDir: directorySchema.nullable().optional(),
  codexConfigDir: directorySchema.nullable().optional(),
  geminiConfigDir: directorySchema.nullable().optional(),

  // 当前供应商 ID（设备级）
  currentProviderClaude: z.string().optional(),
  currentProviderCodex: z.string().optional(),
  currentProviderGemini: z.string().optional(),

  // 目录多组设置
  configDirectorySets: z.array(configDirectorySetSchema).optional(),
  activeConfigDirectorySetId: z.string().optional(),
});

export type SettingsFormData = z.infer<typeof settingsSchema>;
