import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";
import type { CodebuddyQuota } from "@/types/subscription";

/** 和组件里的 t 一样处理 defaultValue 和插值，便于断言最终文案 */
const t = ((key: string, options?: Record<string, unknown>) => {
  const template =
    typeof options?.defaultValue === "string" ? options.defaultValue : key;
  return template.replace(/\{\{(\w+)\}\}/g, (_match, name: string) =>
    String(options?.[name] ?? ""),
  );
}) as unknown as TFunction;

const quota = (overrides: Partial<CodebuddyQuota> = {}): CodebuddyQuota => ({
  success: true,
  error: null,
  queried_at: 1_700_000_000,
  total: 1000,
  remaining: 665.26,
  remaining_percent: 67,
  packages: [
    {
      name: "个人套餐",
      total: 1000,
      remaining: 665.26,
      used: 334.74,
      cycle_start: "2026-10-01 00:00:00",
      cycle_end: "2026-11-01 00:00:00",
    },
  ],
  ...overrides,
});

describe("codebuddyQuotaState", () => {
  it("maps credits to one row with the backend percentage and cycle end", async () => {
    const { codebuddyQuotaState } = await import(
      "@/components/settings/auth/AccountQuota"
    );

    const state = codebuddyQuotaState(t, quota(), false, "zh");
    expect(state?.kind).toBe("rows");
    if (state?.kind !== "rows") return;

    expect(state.rows).toHaveLength(1);
    const [row] = state.rows;
    expect(row.label).toBe("CodeBuddy 额度");
    expect(row.line.left).toBe(67);
    expect(row.line.tone).toBe("normal");
    expect(row.line.value).toBe("665.26");
    expect(row.line.text).toBe("剩余 665.26 / 1,000 credits");
    expect(row.line.resetsAt).toBe(
      new Date("2026-11-01T00:00:00").toISOString(),
    );
  });

  it("flags a nearly exhausted quota as warning and an exhausted one as danger", async () => {
    const { codebuddyQuotaState } = await import(
      "@/components/settings/auth/AccountQuota"
    );

    const low = codebuddyQuotaState(
      t,
      quota({ remaining: 100, remaining_percent: 10 }),
      false,
      "zh",
    );
    expect(low?.kind).toBe("rows");
    if (low?.kind === "rows") {
      expect(low.rows[0].line.left).toBe(10);
      expect(low.rows[0].line.tone).toBe("warning");
    }

    const empty = codebuddyQuotaState(
      t,
      quota({ remaining: 0, remaining_percent: 0, packages: [] }),
      false,
      "zh",
    );
    expect(empty?.kind).toBe("rows");
    if (empty?.kind === "rows") {
      expect(empty.rows[0].line.left).toBe(0);
      expect(empty.rows[0].line.tone).toBe("danger");
      // 没有资源包时没有重置时间
      expect(empty.rows[0].line.resetsAt).toBeNull();
    }
  });

  it("accepts a millisecond epoch cycle end", async () => {
    const { codebuddyQuotaState } = await import(
      "@/components/settings/auth/AccountQuota"
    );
    const cycleEnd = 1_780_000_000_000;
    const state = codebuddyQuotaState(
      t,
      quota({
        packages: [
          {
            name: "个人套餐",
            total: 1000,
            remaining: 665.26,
            used: 334.74,
            cycle_start: null,
            cycle_end: String(cycleEnd),
          },
        ],
      }),
      false,
      "zh",
    );
    expect(state?.kind).toBe("rows");
    if (state?.kind === "rows") {
      expect(state.rows[0].line.resetsAt).toBe(
        new Date(cycleEnd).toISOString(),
      );
    }
  });

  it("shows the backend reason when the quota query failed", async () => {
    const { codebuddyQuotaState } = await import(
      "@/components/settings/auth/AccountQuota"
    );
    expect(
      codebuddyQuotaState(
        t,
        quota({ success: false, error: "登录已过期" }),
        false,
        "zh",
      ),
    ).toEqual({ kind: "failed", reason: "登录已过期" });
    expect(
      codebuddyQuotaState(
        t,
        quota({ success: false, error: null }),
        false,
        "zh",
      ),
    ).toEqual({ kind: "failed", reason: "查询失败" });
  });

  it("reports loading only while the first query is in flight", async () => {
    const { codebuddyQuotaState } = await import(
      "@/components/settings/auth/AccountQuota"
    );
    expect(codebuddyQuotaState(t, undefined, true, "zh")).toEqual({
      kind: "loading",
    });
    expect(codebuddyQuotaState(t, undefined, false, "zh")).toBeNull();
  });
});
