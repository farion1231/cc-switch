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
  language: z.enum(["en", "zh", "ja"]).optional(),

  // 设备级目录覆盖
  claudeConfigDir: directorySchema.nullable().optional(),
  codexConfigDir: directorySchema.nullable().optional(),
  geminiConfigDir: directorySchema.nullable().optional(),

  // 当前供应商 ID（设备级）
  currentProviderClaude: z.string().optional(),
  currentProviderCodex: z.string().optional(),
  currentProviderGemini: z.string().optional(),

  // 键盘快捷键设置
  searchShortcut: z.string().optional(), // 默认: "mod+k" (mod = Mac 上的 Cmd, Windows 上的 Ctrl)
});

export type SettingsFormData = z.infer<typeof settingsSchema>;
