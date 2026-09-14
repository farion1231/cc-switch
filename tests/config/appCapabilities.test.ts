import { describe, expect, it } from "vitest";
import { getAppCapabilities, supportsAppView } from "@/config/appCapabilities";

describe("DeepSeek Harness app capabilities", () => {
  it("uses the live provider registry and disables unrelated integrations", () => {
    const capabilities = getAppCapabilities("dsh");
    expect(capabilities.providerMode).toBe("dsh-live");
    expect(capabilities.defaultModel).toBe(true);
    expect(capabilities.proxy).toBe(false);
    expect(capabilities.mcp).toBe(false);
    expect(capabilities.skills).toBe(false);
    expect(capabilities.sessions).toBe(false);
    expect(capabilities.usage).toBe(false);
    expect(capabilities.tray).toBe(false);
  });

  it("keeps Claude Desktop shared Claude surfaces available", () => {
    const capabilities = getAppCapabilities("claude-desktop");

    expect(capabilities).toMatchObject({
      providerMode: "generic",
      profiles: true,
      prompts: true,
      skills: true,
      mcp: true,
      sessions: true,
      failover: false,
      universal: false,
      usage: true,
    });
    expect(supportsAppView("claude-desktop", "prompts")).toBe(true);
    expect(supportsAppView("claude-desktop", "skills")).toBe(true);
    expect(supportsAppView("claude-desktop", "mcp")).toBe(true);
    expect(supportsAppView("claude-desktop", "sessions")).toBe(true);
    expect(supportsAppView("claude-desktop", "universal")).toBe(false);
  });

  it("only permits provider and settings views for DSH", () => {
    expect(supportsAppView("dsh", "providers")).toBe(true);
    expect(supportsAppView("dsh", "settings")).toBe(true);
    expect(supportsAppView("dsh", "mcp")).toBe(false);
    expect(supportsAppView("dsh", "skills")).toBe(false);
    expect(supportsAppView("dsh", "sessions")).toBe(false);
    expect(supportsAppView("dsh", "universal")).toBe(false);
  });
});
