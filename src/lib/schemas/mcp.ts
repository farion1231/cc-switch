import { z } from "zod";
import { getActiveLanguage } from "@/lib/locale";

function t(messages: Record<"zh" | "en" | "ja" | "ru", string>): string {
  return messages[getActiveLanguage()] ?? messages.en;
}

const mcpServerSpecSchema = z
  .object({
    type: z.enum(["stdio", "http", "sse"]).optional(),
    command: z.string().trim().optional(),
    args: z.array(z.string()).optional(),
    env: z.record(z.string(), z.string()).optional(),
    cwd: z.string().optional(),
    url: z
      .string()
      .trim()
      .url(
        t({
          zh: "请输入有效的 URL",
          en: "Please enter a valid URL",
          ja: "有効な URL を入力してください",
          ru: "Введите действительный URL",
        }),
      )
      .optional(),
    headers: z.record(z.string(), z.string()).optional(),
  })
  .superRefine((server, ctx) => {
    const type = server.type ?? "stdio";
    if (type === "stdio" && !server.command?.trim()) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: t({
          zh: "stdio 类型需填写 command",
          en: "stdio type requires command",
          ja: "stdio では command が必要です",
          ru: "Для типа stdio требуется command",
        }),
        path: ["command"],
      });
    }
    if ((type === "http" || type === "sse") && !server.url?.trim()) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: t({
          zh: `${type} 类型需填写 url`,
          en: `${type} type requires url`,
          ja: `${type} では url が必要です`,
          ru: `Для типа ${type} требуется url`,
        }),
        path: ["url"],
      });
    }
  });

export const mcpServerSchema = z.object({
  id: z.string().min(
    1,
    t({
      zh: "请输入服务器 ID",
      en: "Please enter the server ID",
      ja: "サーバー ID を入力してください",
      ru: "Введите ID сервера",
    }),
  ),
  name: z.string().optional(),
  description: z.string().optional(),
  tags: z.array(z.string()).optional(),
  homepage: z
    .string()
    .url(
      t({
        zh: "请输入有效的网址",
        en: "Please enter a valid URL",
        ja: "有効な URL を入力してください",
        ru: "Введите действительный URL",
      }),
    )
    .optional(),
  docs: z
    .string()
    .url(
      t({
        zh: "请输入有效的网址",
        en: "Please enter a valid URL",
        ja: "有効な URL を入力してください",
        ru: "Введите действительный URL",
      }),
    )
    .optional(),
  enabled: z.boolean().optional(),
  server: mcpServerSpecSchema,
});

export type McpServerFormData = z.infer<typeof mcpServerSchema>;
