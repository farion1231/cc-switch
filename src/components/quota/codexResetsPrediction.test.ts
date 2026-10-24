import { describe, expect, it } from "vitest";
import type { CodexResetsStatus } from "@/types/subscription";
import {
  codexResetsDetailLines,
  summarizeCodexResets,
} from "./codexResetsPrediction";

const NOW = Date.parse("2026-10-24T12:00:00.000Z");

function status(overrides: Partial<CodexResetsStatus> = {}): CodexResetsStatus {
  return {
    latestReset: {
      id: "observed-1",
      resetType: "regular",
      announcedAt: "2026-10-23T17:00:00.000Z",
      text: null,
      source: { kind: "observed", url: null },
    },
    scheduledReset: null,
    activeWatch: null,
    stats: {
      total: 66,
      lastResetAt: "2026-10-23T17:00:00.000Z",
      daysSinceLast: 0.7,
      avgIntervalDays: 6.2,
    },
    fetchedAt: NOW,
    ...overrides,
  };
}

describe("summarizeCodexResets", () => {
  it("未过期的重置观测优先，带概率", () => {
    const summary = summarizeCodexResets(
      status({
        activeWatch: {
          level: "strong",
          resetChancePercent: 60,
          forecastWindow: "next 6 hours",
          observedAt: "2026-10-24T08:00:00.000Z",
          expiresAt: "2026-10-24T14:00:00.000Z",
          text: null,
          source: { kind: "observed", url: null },
        },
      }),
      NOW,
    );
    expect(summary).toEqual({ kind: "watch", chancePercent: 60 });
  });

  it("已过期的观测不算数，落到未来的常规重置", () => {
    const summary = summarizeCodexResets(
      status({
        scheduledReset: {
          id: "x-1",
          resetType: "regular",
          announcedAt: "2026-10-24T09:00:00.000Z",
          scheduledFor: "2026-10-25T01:00:00.000Z",
          text: null,
          source: { kind: "x_post", url: null },
        },
        activeWatch: {
          level: "elevated",
          resetChancePercent: null,
          forecastWindow: "next 2 hours",
          observedAt: "2026-10-24T06:00:00.000Z",
          expiresAt: "2026-10-24T08:00:00.000Z",
          text: null,
          source: { kind: "observed", url: null },
        },
      }),
      NOW,
    );
    expect(summary).toEqual({
      kind: "scheduled",
      resetsAt: "2026-10-25T01:00:00.000Z",
    });
  });

  it("预定时刻已过的公告不算待执行，回到没有信号", () => {
    const summary = summarizeCodexResets(
      status({
        scheduledReset: {
          id: "x-2",
          resetType: "banked",
          announcedAt: "2026-10-23T09:00:00.000Z",
          scheduledFor: "2026-10-24T01:00:00.000Z",
          text: null,
          source: { kind: "x_post", url: null },
        },
      }),
      NOW,
    );
    expect(summary).toEqual({ kind: "noSignal" });
  });

  it("什么都没有时是没有信号", () => {
    expect(summarizeCodexResets(status({ latestReset: null }), NOW)).toEqual({
      kind: "noSignal",
    });
  });
});

describe("codexResetsDetailLines", () => {
  it("上次重置、待执行的常规重置、进行中的观测各占一行，过期观测不列", () => {
    const lines = codexResetsDetailLines(
      status({
        scheduledReset: {
          id: "x-1",
          resetType: "regular",
          announcedAt: "2026-10-24T09:00:00.000Z",
          scheduledFor: "2026-10-25T01:00:00.000Z",
          text: null,
          source: { kind: "x_post", url: null },
        },
        activeWatch: {
          level: "strong",
          resetChancePercent: 55,
          forecastWindow: "next 6 hours",
          observedAt: "2026-10-24T08:00:00.000Z",
          expiresAt: "2026-10-24T14:00:00.000Z",
          text: null,
          source: { kind: "observed", url: null },
        },
      }),
      NOW,
    );
    expect(lines.map((line) => line.key)).toEqual([
      "lastReset",
      "scheduled",
      "watch",
    ]);
    expect(lines[2].chancePercent).toBe(55);
  });

  it("只有上次重置时也只列那一行", () => {
    const lines = codexResetsDetailLines(status(), NOW);
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatchObject({
      key: "lastReset",
      at: "2026-10-23T17:00:00.000Z",
      resetType: "regular",
    });
  });
});
