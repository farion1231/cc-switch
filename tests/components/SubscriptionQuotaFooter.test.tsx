import { render, screen } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import zh from "@/i18n/locales/zh.json";
import type { SubscriptionQuota } from "@/types/subscription";

const quota = (
  overrides: Partial<SubscriptionQuota> = {},
): SubscriptionQuota => ({
  tool: "codex_oauth",
  credentialStatus: "valid",
  credentialMessage: null,
  success: true,
  tiers: [],
  extraUsage: null,
  error: null,
  queriedAt: null,
  ...overrides,
});

function renderQuota(value: SubscriptionQuota, inline = false) {
  return render(
    <SubscriptionQuotaView
      quota={value}
      loading={false}
      refetch={vi.fn()}
      appIdForExpiredHint="codex_oauth"
      inline={inline}
    />,
  );
}

describe("SubscriptionQuotaView Codex reset cards", () => {
  beforeEach(async () => {
    i18n.addResourceBundle("zh", "translation", zh, true, true);
    await i18n.changeLanguage("zh");
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-01T00:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders plan and card details even when no usage tier is present", () => {
    renderQuota(
      quota({
        planType: "plus",
        rateLimitResetCredits: {
          availableCount: 2,
          credits: [
            {
              grantedAt: "2026-06-30T00:00:00Z",
              expiresAt: "2026-07-31T00:00:00Z",
              title: "全量重置",
              description: "重置当前用量窗口",
            },
          ],
        },
      }),
    );

    expect(screen.getByText("订阅额度")).toBeInTheDocument();
    expect(screen.getByText("Plus")).toBeInTheDocument();
    expect(screen.getByText("重置卡")).toBeInTheDocument();
    expect(screen.getByText("2 张可用")).toBeInTheDocument();
    expect(screen.getByText("全量重置")).toBeInTheDocument();
    expect(screen.getByText("重置当前用量窗口")).toBeInTheDocument();
    expect(screen.getByText(/后到期/)).toBeInTheDocument();
  });

  it("renders an account-bound membership renewal even without usage tiers", () => {
    renderQuota(
      quota({
        planType: "plus",
        membership: {
          activeUntil: "2026-07-31T00:00:00Z",
          willRenew: true,
        },
      }),
    );

    expect(screen.getByText("下次续费")).toBeInTheDocument();
    expect(screen.getByText(/30d0h/)).toBeInTheDocument();
    expect(screen.getByText(/ChatGPT 订阅元数据/)).toBeInTheDocument();
  });

  it("distinguishes a non-renewing membership from an expired record", () => {
    const { rerender } = renderQuota(
      quota({
        membership: {
          activeUntil: "2026-07-02T00:00:00Z",
          willRenew: false,
        },
      }),
      true,
    );
    expect(screen.getByText("到期:")).toBeInTheDocument();
    expect(screen.getByText("24h0m")).toBeInTheDocument();

    rerender(
      <SubscriptionQuotaView
        quota={quota({
          membership: {
            activeUntil: "2026-06-30T00:00:00Z",
            willRenew: null,
          },
        })}
        loading={false}
        refetch={vi.fn()}
        appIdForExpiredHint="codex_oauth"
        inline
      />,
    );
    expect(screen.getByText("已到期:")).toBeInTheDocument();
    expect(screen.getByText("已到期")).toBeInTheDocument();
  });

  it("keeps the authoritative count when card details are unavailable", () => {
    renderQuota(
      quota({
        rateLimitResetCredits: { availableCount: 1, credits: null },
      }),
    );

    expect(screen.getByText("1 张可用")).toBeInTheDocument();
    expect(
      screen.getByText("卡片明细暂不可用，数量仍为官方额度接口返回值"),
    ).toBeInTheDocument();
  });

  it("distinguishes a known zero from an unsupported reset-card field", () => {
    const { rerender } = renderQuota(
      quota({
        rateLimitResetCredits: { availableCount: 0, credits: null },
      }),
    );
    expect(screen.getByText("0 张可用")).toBeInTheDocument();

    rerender(
      <SubscriptionQuotaView
        quota={quota()}
        loading={false}
        refetch={vi.fn()}
        appIdForExpiredHint="codex_oauth"
      />,
    );
    expect(screen.queryByText("重置卡")).not.toBeInTheDocument();
  });

  it("shows a compact count and nearest expiry in inline mode", () => {
    renderQuota(
      quota({
        planType: "team_business",
        rateLimitResetCredits: {
          availableCount: 3,
          credits: [
            {
              grantedAt: null,
              expiresAt: "2026-07-03T00:00:00Z",
              title: null,
              description: null,
            },
            {
              grantedAt: null,
              expiresAt: "2026-07-02T00:00:00Z",
              title: null,
              description: null,
            },
          ],
        },
      }),
      true,
    );

    expect(screen.getByText("Team Business")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("24h0m")).toBeInTheDocument();
  });

  it("preserves the legacy tier-only rendering", () => {
    renderQuota(
      quota({
        tiers: [
          {
            name: "five_hour",
            utilization: 25,
            resetsAt: "2026-07-01T05:00:00Z",
          },
        ],
      }),
    );

    expect(screen.getByText("5小时")).toBeInTheDocument();
    expect(screen.getByText("25%")).toBeInTheDocument();
    expect(screen.queryByText("重置卡")).not.toBeInTheDocument();
  });
});
