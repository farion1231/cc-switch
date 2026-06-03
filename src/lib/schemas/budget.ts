import { z } from "zod";

export const budgetSchema = z
  .object({
    name: z.string().min(1, "budget.validation.nameRequired"),
    scope: z.enum(["global", "app", "provider", "model"]),
    scopeValue: z.string().optional(),
    period: z.enum(["daily", "weekly", "monthly"]),
    periodStartDay: z.number().default(1),
    limitTokens: z
      .number()
      .positive("budget.validation.tokensPositive")
      .optional(),
    limitUsd: z
      .string()
      .optional()
      .refine(
        (val) => val === undefined || val === "" || /^\d+(\.\d+)?$/.test(val),
        "budget.validation.invalidUsd",
      )
      .refine(
        (val) => val === undefined || val === "" || parseFloat(val) > 0,
        "budget.validation.usdPositive",
      ),
    enabled: z.boolean().default(true),
  })
  .superRefine((data, ctx) => {
    // scope ≠ "global" 时 scopeValue 必填
    if (
      data.scope !== "global" &&
      (!data.scopeValue || data.scopeValue.trim() === "")
    ) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: "budget.validation.scopeValueRequired",
        path: ["scopeValue"],
      });
    }

    // limitTokens 和 limitUsd 至少设一个
    const hasTokens = data.limitTokens !== undefined && data.limitTokens > 0;
    const hasUsd =
      data.limitUsd !== undefined &&
      data.limitUsd !== "" &&
      parseFloat(data.limitUsd) > 0;
    if (!hasTokens && !hasUsd) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: "budget.validation.atLeastOneLimit",
        path: ["limitTokens"],
      });
    }

    // periodStartDay 范围校验
    if (
      data.period === "weekly" &&
      (data.periodStartDay < 0 || data.periodStartDay > 6)
    ) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: "budget.validation.startDayWeekly",
        path: ["periodStartDay"],
      });
    }
    if (
      data.period === "monthly" &&
      (data.periodStartDay < 1 || data.periodStartDay > 28)
    ) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: "budget.validation.startDayMonthly",
        path: ["periodStartDay"],
      });
    }
  });

export type BudgetFormData = z.infer<typeof budgetSchema>;
