import { z } from "zod";

const mcpServerSpecSchema = z
  .object({
    type: z.enum(["stdio", "http", "sse"]).optional(),
    command: z.string().trim().optional(),
    args: z.array(z.string()).optional(),
    env: z.record(z.string(), z.string()).optional(),
    cwd: z.string().optional(),
    url: z.string().trim().url("请输入有效的 URL").optional(),
    headers: z.record(z.string(), z.string()).optional(),
  })
  .superRefine((server, ctx) => {
    const type = server.type ?? "stdio";
    if (type === "stdio" && !server.command?.trim()) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: "stdio 类型需填写 command",
        path: ["command"],
      });
    }
    if ((type === "http" || type === "sse") && !server.url?.trim()) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: `${type} 类型需填写 url`,
        path: ["url"],
      });
    }
  });

export const mcpServerSchema = z.object({
  id: z.string().min(1, "请输入服务器 ID"),
  name: z.string().optional(),
  description: z.string().optional(),
  tags: z.array(z.string()).optional(),
  homepage: z.string().url().optional(),
  docs: z.string().url().optional(),
  enabled: z.boolean().optional(),
  server: mcpServerSpecSchema,
});

export type McpServerFormData = z.infer<typeof mcpServerSchema>;
