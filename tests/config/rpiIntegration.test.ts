import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { APP_ICON_MAP } from "@/config/appConfig";
import {
  extractBaseUrlForUsageDetection,
  injectCodingPlanUsageScript,
} from "@/config/codingPlanProviders";
import {
  invalidatePiDirectoryCaches,
  invalidatePiProviderCaches,
} from "@/lib/query/pi";
import { PROMPT_APP_IDS } from "@/lib/query/prompts";
import { translatePiProviderMutationError } from "@/utils/errorUtils";

describe("rpi shared UI integration", () => {
  it("uses the lowercase brand and includes it in shared prompts", () => {
    expect(APP_DISPLAY_NAME.rpi).toBe("rpi");
    expect(APP_ICON_MAP.rpi.label).toBe("rpi");
    expect(PROMPT_APP_IDS).toContain("rpi");
  });

  it("detects the same Coding Plan endpoint as Pi", () => {
    const provider = {
      settingsConfig: { baseUrl: "https://api.kimi.com/coding/v1" },
    };
    expect(
      extractBaseUrlForUsageDetection("rpi", provider.settingsConfig),
    ).toBe(provider.settingsConfig.baseUrl);
    const rpi = injectCodingPlanUsageScript("rpi", provider);
    expect(rpi).toMatchObject({ meta: { usage_script: { enabled: true } } });
    expect(rpi).toEqual(injectCodingPlanUsageScript("pi", provider));
  });

  it("invalidates rpi provider and directory caches, never Pi", async () => {
    const client = new QueryClient();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await invalidatePiProviderCaches(client, "rpi");
    expect(invalidate.mock.calls.map(([arg]) => arg?.queryKey)).toEqual([
      ["rpi", "currentState"],
      ["providers", "rpi"],
    ]);
    invalidate.mockClear();
    await invalidatePiDirectoryCaches(client, "rpi");
    expect(invalidate.mock.calls.map(([arg]) => arg?.queryKey)).toContainEqual([
      "rpi",
    ]);
    expect(
      invalidate.mock.calls.map(([arg]) => arg?.queryKey),
    ).not.toContainEqual(["pi"]);
  });

  it("refreshes directory-dependent prompt and scan caches without touching Pi", async () => {
    const client = new QueryClient();
    for (const key of [
      ["prompts", "location", "rpi"],
      ["prompts", "list", "rpi"],
      ["skills", "unmanaged"],
      ["prompts", "location", "pi"],
      ["prompts", "list", "pi"],
    ]) {
      client.setQueryData(key, "cached");
    }
    await invalidatePiDirectoryCaches(client, "rpi");
    expect(
      client.getQueryState(["prompts", "location", "rpi"])?.isInvalidated,
    ).toBe(true);
    expect(
      client.getQueryState(["prompts", "list", "rpi"])?.isInvalidated,
    ).toBe(true);
    expect(client.getQueryState(["skills", "unmanaged"])?.isInvalidated).toBe(
      true,
    );
    expect(
      client.getQueryState(["prompts", "location", "pi"])?.isInvalidated,
    ).toBe(false);
    expect(client.getQueryState(["prompts", "list", "pi"])?.isInvalidated).toBe(
      false,
    );
  });

  it("uses rpi's conflict notice", () => {
    expect(
      translatePiProviderMutationError(
        "models.json changed",
        (key) => key,
        "rpi",
      ),
    ).toBe("rpi.provider.writeConflict");
  });
});
