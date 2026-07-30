import { describe, expect, it } from "vitest";
import {
  APP_CAPABILITIES,
  APP_IDS,
  canShowUsageDashboard,
} from "@/config/appConfig";

describe("APP_CAPABILITIES", () => {
  it("为每个 AppId 提供完整能力配置", () => {
    expect(Object.keys(APP_CAPABILITIES).sort()).toEqual([...APP_IDS].sort());
  });

  it("将 Cursor 隔离在通用受管理应用状态机之外", () => {
    expect(APP_CAPABILITIES.cursor).toMatchObject({
      providerCatalog: true,
      providerFlow: "cursor-runtime",
      routingControl: "local-runtime",
      managedAppId: null,
      sharedFeatureAppId: null,
      usageDashboard: true,
      prompts: false,
      skills: false,
      mcp: false,
      sessions: false,
      profiles: false,
      environmentConflictCheck: false,
      failover: false,
    });
  });

  it("只让 managed provider flow 暴露 ManagedAppId", () => {
    for (const [appId, capabilities] of Object.entries(APP_CAPABILITIES)) {
      if (capabilities.providerFlow === "managed") {
        expect(capabilities.managedAppId, appId).toBe(appId);
      } else {
        expect(capabilities.managedAppId, appId).toBeNull();
      }
    }
  });

  it("显式声明 Claude Desktop 的共享功能目标", () => {
    expect(APP_CAPABILITIES["claude-desktop"].managedAppId).toBe(
      "claude-desktop",
    );
    expect(APP_CAPABILITIES["claude-desktop"].sharedFeatureAppId).toBe(
      "claude",
    );
  });

  it("将 usageDashboard 作为使用统计入口的总开关", () => {
    expect(canShowUsageDashboard(APP_CAPABILITIES.cursor, false)).toBe(true);
    expect(canShowUsageDashboard(APP_CAPABILITIES.claude, false)).toBe(false);
    expect(canShowUsageDashboard(APP_CAPABILITIES.claude, true)).toBe(true);
    expect(
      canShowUsageDashboard(
        { ...APP_CAPABILITIES.cursor, usageDashboard: false },
        true,
      ),
    ).toBe(false);
  });
});
