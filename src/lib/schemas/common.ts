import { z } from "zod";
import { validateToml, tomlToMcpServer } from "@/utils/tomlUtils";
import { getActiveLanguage } from "@/lib/locale";

function t(messages: Record<"zh" | "en" | "ja" | "ru", string>): string {
  return messages[getActiveLanguage()] ?? messages.en;
}

/**
 * 解析 JSON 语法错误，返回更友好的位置信息。
 */
function parseJsonError(error: unknown): string {
  if (!(error instanceof SyntaxError)) {
    return t({
      zh: "JSON 格式错误",
      en: "Invalid JSON format",
      ja: "JSON 形式が無効です",
      ru: "Неверный формат JSON",
    });
  }

  const message = error.message || "JSON 解析失败";

  // Chrome/V8: "Unexpected token ... in JSON at position 123"
  const positionMatch = message.match(/at position (\d+)/i);
  if (positionMatch) {
    const position = parseInt(positionMatch[1], 10);
    return t({
      zh: `JSON 格式错误（位置：${position}）`,
      en: `Invalid JSON format (position: ${position})`,
      ja: `JSON 形式が無効です（位置: ${position}）`,
      ru: `Неверный формат JSON (позиция: ${position})`,
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

  return t({
    zh: `JSON 格式错误：${message}`,
    en: `Invalid JSON format: ${message}`,
    ja: `JSON 形式が無効です: ${message}`,
    ru: `Неверный формат JSON: ${message}`,
  });
}

/**
 * 通用的 JSON 配置文本校验：
 * - 非空
 * - 可解析且为对象（非数组）
 */
export const jsonConfigSchema = z
  .string()
  .min(
    1,
    t({
      zh: "配置不能为空",
      en: "Configuration cannot be empty",
      ja: "設定は空にできません",
      ru: "Конфигурация не может быть пустой",
    }),
  )
  .superRefine((value, ctx) => {
    try {
      const obj = JSON.parse(value);
      if (!obj || typeof obj !== "object" || Array.isArray(obj)) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          message: t({
            zh: "需为单个对象配置",
            en: "Configuration must be a single object",
            ja: "設定は単一のオブジェクトである必要があります",
            ru: "Конфигурация должна быть одним объектом",
          }),
        });
      }
    } catch (e) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: parseJsonError(e),
      });
    }
  });

/**
 * 通用的 TOML 配置文本校验：
 * - 允许为空（由上层业务决定是否必填）
 * - 语法与结构有效
 * - 针对 stdio/http/sse 的必填字段（command/url）进行提示
 */
export const tomlConfigSchema = z.string().superRefine((value, ctx) => {
  const err = validateToml(value);
  if (err) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      message: t({
        zh: `TOML 无效：${err}`,
        en: `Invalid TOML: ${err}`,
        ja: `TOML が無効です: ${err}`,
        ru: `Неверный TOML: ${err}`,
      }),
    });
    return;
  }

  if (!value.trim()) return;

  try {
    const server = tomlToMcpServer(value);
    if (server.type === "stdio" && !server.command?.trim()) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: t({
          zh: "stdio 类型需填写 command",
          en: "stdio type requires command",
          ja: "stdio では command が必要です",
          ru: "Для типа stdio требуется command",
        }),
      });
    }
    if (
      (server.type === "http" || server.type === "sse") &&
      !server.url?.trim()
    ) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: t({
          zh: `${server.type} 类型需填写 url`,
          en: `${server.type} type requires url`,
          ja: `${server.type} では url が必要です`,
          ru: `Для типа ${server.type} требуется url`,
        }),
      });
    }
  } catch (e: any) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      message:
        e?.message ||
        t({
          zh: "TOML 解析失败",
          en: "Failed to parse TOML",
          ja: "TOML の解析に失敗しました",
          ru: "Не удалось разобрать TOML",
        }),
    });
  }
});
