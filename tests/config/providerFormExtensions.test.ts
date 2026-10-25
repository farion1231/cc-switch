import { describe, expect, it } from "vitest";
import {
  getFormExtensionSaveMeta,
  registerFormExtension,
} from "@/config/formExtensions";
import { huaweiFormBehavior } from "@/huawei/formBehavior";

describe("provider save policy", () => {
  it("leaves unregistered providers untouched", () => {
    expect(
      getFormExtensionSaveMeta("ordinary", {
        appId: "claude",
        selectedAccountId: "other",
      }),
    ).toEqual({});
  });

  it("lets any registered provider supply save metadata", () => {
    registerFormExtension({
      providerType: "test-extension",
      onSaveMeta: ({ appId }) => ({ extensionTarget: appId }),
    });
    expect(
      getFormExtensionSaveMeta("test-extension", { appId: "claude-desktop" }),
    ).toEqual({ extensionTarget: "claude-desktop" });
  });

  it("keeps CLI and Desktop targets separate and supports clearing an account", () => {
    registerFormExtension({
      providerType: "test-huawei-policy",
      ...huaweiFormBehavior,
    });
    expect(
      getFormExtensionSaveMeta("test-huawei-policy", {
        appId: "claude",
        selectedAccountId: "account-a",
      }),
    ).toEqual({ appTarget: "claude", selectedAccountId: "account-a" });
    const desktop = getFormExtensionSaveMeta("test-huawei-policy", {
      appId: "claude-desktop",
      presetAppTarget: "claude",
      selectedAccountId: null,
    });
    expect(desktop.appTarget).toBe("claude_desktop");
    expect(
      { ...{ selectedAccountId: "old-account" }, ...desktop }.selectedAccountId,
    ).toBeUndefined();
  });
});
