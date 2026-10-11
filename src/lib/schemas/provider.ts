import { z } from "zod";

/**
 * 解析 JSON 语法错误，提取位置信息
 */
function parseJsonError(error: unknown): string {
  if (!(error instanceof SyntaxError)) {
    return "配置 JSON 格式错误";
  }

  const message = error.message;

  // 提取位置信息：Chrome/V8: "Unexpected token ... in JSON at position 123"
  const positionMatch = message.match(/at position (\d+)/i);
  if (positionMatch) {
    const position = parseInt(positionMatch[1], 10);
    return `JSON 格式错误：${message.split(" in JSON")[0]}（位置：${position}）`;
  }

  // Firefox: "JSON.parse: unexpected character at line 1 column 23"
  const lineColumnMatch = message.match(/line (\d+) column (\d+)/i);
  if (lineColumnMatch) {
    const line = lineColumnMatch[1];
    const column = lineColumnMatch[2];
    return `JSON 格式错误：第 ${line} 行，第 ${column} 列`;
  }

  // 通用情况：提取关键错误信息
  const cleanMessage = message
    .replace(/^JSON\.parse:\s*/i, "")
    .replace(/^Unexpected\s+/i, "意外的 ")
    .replace(/token/gi, "符号")
    .replace(/Expected/gi, "预期");

  return `JSON 格式错误：${cleanMessage}`;
}

// 自定义图标来源（issue #8117）
export const ICON_SOURCES = ["builtin", "local", "url"] as const;
export type IconSource = (typeof ICON_SOURCES)[number];

// "local" 时 icon 存资产键（{provider_stem}-{hash8}.{ext}），"url" 时存 http(s) 图片 URL
const LOCAL_ICON_KEY_PATTERN =
  /^[\w-]{1,80}-[0-9a-f]{8}\.(png|jpe?g|webp|svg)$/;

export const providerSchema = z
  .object({
    name: z.string(), // 必填校验移至 handleSubmit 中用 toast 提示
    websiteUrl: z.string().url("请输入有效的网址").optional().or(z.literal("")),
    notes: z.string().optional(),
    settingsConfig: z
      .string()
      .min(1, "请填写配置内容")
      .superRefine((value, ctx) => {
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
    iconSource: z.enum(ICON_SOURCES).optional(),
  })
  // iconSource 与 icon 的组合约束：旧数据（无 iconSource）原样通过
  .superRefine((data, ctx) => {
    if (!data.iconSource) return;
    const icon = data.icon ?? "";
    if (data.iconSource === "url") {
      if (!/^https?:\/\/\S+$/i.test(icon)) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["icon"],
          message: "自定义 URL 图标需要有效的 http(s) 图片地址",
        });
      }
      return;
    }
    if (data.iconSource === "local" && !LOCAL_ICON_KEY_PATTERN.test(icon)) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["icon"],
        message: "本地图标资产键格式不正确",
      });
    }
  });

export type ProviderFormData = z.infer<typeof providerSchema>;
