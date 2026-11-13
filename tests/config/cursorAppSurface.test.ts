import { describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import { APP_IDS, ROUTE_MODE_APP_IDS, appHas } from "@/config/appSpecs";
import { APP_ICON_MAP, DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { DIRECTORY_APPS } from "@/config/directoryApps";
import { SESSION_APP_IDS } from "@/components/sessions/utils";
import {
  buildAdditiveSections,
  type AdditiveInput,
} from "@/components/providers/presentation";
import { providerNeedsRouting } from "@/utils/providerCapabilities";
import type { Provider } from "@/types";

const t = ((key: string, options?: Record<string, unknown>) =>
  options && "count" in options
    ? `${key}:${String(options.count)}`
    : key) as unknown as TFunction;

const officialCursor: Provider = {
  id: "cursor-official",
  name: "Cursor",
  category: "official",
  settingsConfig: {},
};

const thirdPartyCursor: Provider = {
  id: "cursor-third-party",
  name: "Third party",
  category: "custom",
  settingsConfig: {},
};

function cursorSections(providers: Provider[]) {
  const input: AdditiveInput = {
    app: "cursor",
    t,
    providers,
    isInConfig: () => true,
    actions: {
      add: vi.fn(),
      remove: vi.fn(),
      disableOmo: vi.fn(),
      setDefault: vi.fn(),
    },
  };
  return buildAdditiveSections(input);
}

// Cursor 只作为「被接入的应用」存在：本地路由 / 路由接管属于被拆走的后端一半，
// 前端不能再出现这些入口，否则用户会点到一条没有后端支撑的路。
describe("Cursor app: supported surface", () => {
  it("is an opt-in app that keeps the shared surfaces", () => {
    expect(APP_IDS).toContain("cursor");
    expect(APP_ICON_MAP.cursor).toBeDefined();
    // 与后端默认值一致：Cursor 默认不在侧栏里，用户自己打开
    expect(DEFAULT_VISIBLE_APPS.cursor).toBe(false);
    expect(DIRECTORY_APPS.some((entry) => entry.app === "cursor")).toBe(true);
    expect(SESSION_APP_IDS).toContain("cursor");
    expect(appHas("cursor", "mcp")).toBe(true);
    expect(appHas("cursor", "skills")).toBe(true);
    expect(appHas("cursor", "prompts")).toBe(true);
  });

  it("declares neither a local proxy nor a route mode", () => {
    expect(appHas("cursor", "localProxy")).toBe(false);
    expect(appHas("cursor", "routeMode")).toBe(false);
    expect(ROUTE_MODE_APP_IDS).not.toContain("cursor");
  });

  it("never asks the user to route requests", () => {
    expect(providerNeedsRouting("cursor", officialCursor)).toBe(false);
    expect(providerNeedsRouting("cursor", thirdPartyCursor)).toBe(false);
  });

  it("shows the official provider as a read-only card with no route chips", () => {
    const sections = cursorSections([officialCursor]);
    const card = sections.flatMap((section) => section.items)[0].presentation;

    expect(card.chips?.map((chip) => chip.key)).not.toContain("needsRoute");
    expect(card.editDisabledReason).toBeTruthy();
    expect(card.deleteDisabledReason).toBeTruthy();
  });
});
