import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

const requiredPaths = [
  "common.enableAllForApp",
  "common.disableAllForApp",
  "common.bulkToggleFailed",
  "skills.installedSearchPlaceholder",
  "skills.installedSearchAriaLabel",
  "skills.noInstalledSearchResults",
  "mcp.unifiedPanel.searchPlaceholder",
  "mcp.unifiedPanel.searchAriaLabel",
  "mcp.unifiedPanel.noSearchResults",
  "prompts.searchPlaceholder",
  "prompts.searchAriaLabel",
  "prompts.noSearchResults",
  "sessionManager.usagePrecisionRequest",
  "sessionManager.usagePrecisionSession",
  "sessionManager.usagePrecisionSyncWindow",
  "sessionManager.usagePrecisionEstimated",
  "sessionManager.usagePrecisionUnavailable",
  "sessionManager.usageHttpRequests",
  "sessionManager.usageAssistantMessages",
  "sessionManager.usageAgentCalls",
  "sessionManager.usageUsageEvents",
  "sessionManager.usageEventsUnavailable",
  "sessionManager.usageTokens",
  "sessionManager.usageCost",
  "sessionManager.usageEvents",
  "sessionManager.usagePrecision",
  "sessionManager.usageSyncWindowHint",
  "sessionManager.usageLoadFailed",
  "sessionManager.usageUnavailable",
  "sessionManager.usageUnavailableShort",
  "sessionManager.usagePartial",
  "sessionManager.usageLoading",
  "sessionManager.usageTaskTotal",
  "sessionManager.usageSelf",
  "sessionManager.usageDescendants",
  "sessionManager.usageSyncWindow",
  "sessionManager.usagePartialHint",
  "sessionManager.usageDataDetails",
  "sessionManager.usageDataDetailsClose",
  "sessionManager.usageSessionTimeHint",
  "sessionManager.usageTimeUnavailableHint",
  "usage.taskView",
  "usage.appFilter.claude",
  "usage.appFilter.claudeDesktop",
  "usage.appFilter.codex",
  "usage.appFilter.gemini",
  "usage.appFilter.grokbuild",
  "usage.appFilter.opencode",
  "usage.appFilter.openclaw",
  "usage.appFilter.hermes",
  "usage.appFilter.pi",
  "usage.task.agent",
  "usage.task.allAgents",
  "usage.task.capabilitiesUnavailable",
  "usage.task.clearFilter",
  "usage.task.loadingOptions",
  "usage.task.count",
  "usage.task.count.httpRequest",
  "usage.task.count.assistantMessage",
  "usage.task.count.agentCall",
  "usage.task.count.usageEvent",
  "usage.task.count.unavailable",
  "usage.task.dataStatus",
  "usage.task.descendants",
  "usage.task.empty",
  "usage.task.loadError",
  "usage.task.filterOptionsUnavailable",
  "usage.task.noMeasure",
  "usage.task.noPages",
  "usage.task.pageSize",
  "usage.task.pageSummary",
  "usage.task.partial",
  "usage.task.project",
  "usage.task.projectDir",
  "usage.task.refreshing",
  "usage.task.searchProject",
  "usage.task.searchProjectDir",
  "usage.task.searchTitle",
  "usage.task.selectProject",
  "usage.task.selectTitle",
  "usage.task.self",
  "usage.task.status.available",
  "usage.task.status.partial",
  "usage.task.status.unavailable",
  "usage.task.task",
  "usage.task.title",
  "usage.task.titleUnavailable",
  "usage.task.total",
  "usage.task.totalRecords",
  "usage.task.unavailable",
  "usage.task.viewBreakdown",
  "usage.task.dataDetails",
  "usage.task.dataDetailsClose",
  "usage.task.partialHint",
  "usage.task.syncWindowHint",
  "usage.task.sessionTimeHint",
  "usage.task.timeUnavailableHint",
  "usage.task.precision.requestExact",
  "usage.task.precision.sessionExact",
  "usage.task.precision.syncWindowDelta",
  "usage.task.precision.estimated",
  "usage.task.precision.unavailable",
  "usage.task.time.event",
  "usage.task.time.session",
  "usage.task.time.syncWindow",
  "usage.task.time.unavailable",
] as const;

type Locale = Record<string, unknown>;

const locales = [
  ["en", en],
  ["ja", ja],
  ["zh", zh],
  ["zh-TW", zhTW],
] as const;

function getTranslation(locale: Locale, path: string): unknown {
  const parts = path.split(".");
  let value: unknown = locale;

  for (let index = 0; index < parts.length; index += 1) {
    if (!value || typeof value !== "object") return undefined;

    const record = value as Record<string, unknown>;
    const remainingPath = parts.slice(index).join(".");
    if (Object.prototype.hasOwnProperty.call(record, remainingPath)) {
      return record[remainingPath];
    }

    value = record[parts[index]];
  }

  return value;
}

function interpolationVariables(value: string): string[] {
  return Array.from(
    value.matchAll(/\{\{([^}]+)\}\}/g),
    ([, name]) => name,
  ).sort();
}

describe("management list locale coverage", () => {
  it.each(locales)("defines every management key in %s", (_name, locale) => {
    const missing = requiredPaths.filter((path) => {
      const value = getTranslation(locale as Locale, path);
      return typeof value !== "string" || value.trim().length === 0;
    });

    expect(missing).toEqual([]);
  });

  it.each(locales.slice(1))(
    "preserves interpolation variables in %s",
    (_name, locale) => {
      for (const path of requiredPaths) {
        const expected = getTranslation(en as Locale, path) as string;
        const actual = getTranslation(locale as Locale, path) as string;

        expect(interpolationVariables(actual)).toEqual(
          interpolationVariables(expected),
        );
      }
    },
  );
});
