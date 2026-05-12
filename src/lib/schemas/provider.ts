import { z } from "zod";
import { getActiveLanguage } from "@/lib/locale";

function t(messages: Record<"zh" | "en" | "ja" | "ru", string>): string {
  return messages[getActiveLanguage()] ?? messages.en;
}

/**
 * 解析 JSON 语法错误，提取位置信息
 */
function parseJsonError(error: unknown): string {
  if (!(error instanceof SyntaxError)) {
    return t({
      zh: "配置 JSON 格式错误",
      en: "Configuration JSON format is invalid",
      ja: "設定 JSON 形式が無効です",
      ru: "Неверный формат JSON в конфигурации",
    });
  }

  const message = error.message;

  // 提取位置信息：Chrome/V8: "Unexpected token ... in JSON at position 123"
  const positionMatch = message.match(/at position (\d+)/i);
  if (positionMatch) {
    const position = parseInt(positionMatch[1], 10);
    const detail = message.split(" in JSON")[0];
    return t({
      zh: `JSON 格式错误：${detail}（位置：${position}）`,
      en: `Invalid JSON format: ${detail} (position: ${position})`,
      ja: `JSON 形式が無効です: ${detail}（位置: ${position}）`,
      ru: `Неверный формат JSON: ${detail} (позиция: ${position})`,
    });
  }

  // Firefox: "JSON.parse: unexpected character at line 1 column 23"
  const lineColumnMatch = message.match(/line (\d+) column (\d+)/i);
  if (lineColumnMatch) {
    const line = lineColumnMatch[1];
    const column = lineColumnMatch[2];
    return t({
      zh: `JSON 格式错误：第 ${line} 行，第 ${column} 列`,
      en: `Invalid JSON format: line ${line}, column ${column}`,
      ja: `JSON 形式が無効です: ${line} 行 ${column} 列`,
      ru: `Неверный формат JSON: строка ${line}, столбец ${column}`,
    });
  }

  // 通用情况：提取关键错误信息
  const cleanMessage = message.replace(/^JSON\.parse:\s*/i, "").trim();

  const zhMessage = cleanMessage
    .replace(/^Unexpected\s+/i, "意外的 ")
    .replace(/token/gi, "符号")
    .replace(/Expected/gi, "预期");

  return t({
    zh: `JSON 格式错误：${zhMessage}`,
    en: `Invalid JSON format: ${cleanMessage}`,
    ja: `JSON 形式が無効です: ${cleanMessage}`,
    ru: `Неверный формат JSON: ${cleanMessage}`,
  });
}

export const providerSchema = z.object({
  name: z.string(), // 必填校验移至 handleSubmit 中用 toast 提示
  websiteUrl: z
    .string()
    .optional()
    .or(z.literal(""))
    .superRefine((value, ctx) => {
      const candidate = value ?? "";
      if (!candidate) return;

      try {
        new URL(candidate);
      } catch {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          message: t({
            zh: "请输入有效的网址",
            en: "Please enter a valid URL",
            ja: "有効な URL を入力してください",
            ru: "Введите действительный URL",
          }),
        });
      }
    }),
  notes: z.string().optional(),
  settingsConfig: z.string().superRefine((value, ctx) => {
    if (value.length < 1) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: t({
          zh: "请填写配置内容",
          en: "Please fill in the configuration",
          ja: "設定内容を入力してください",
          ru: "Укажите содержимое конфигурации",
        }),
      });
      return;
    }

    try {
      JSON.parse(value);
    } catch (error) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: parseJsonError(error),
      });
    }
  }),
  // 图标配置
  icon: z.string().optional(),
  iconColor: z.string().optional(),
});

export type ProviderFormData = z.infer<typeof providerSchema>;
