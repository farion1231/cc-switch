import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useApiKeyLink } from "@/components/providers/forms/hooks/useApiKeyLink";

type Props = Parameters<typeof useApiKeyLink>[0];
type Entries = Props["presetEntries"];

const claudePreset = (
  id: string,
  baseUrl: string,
  apiKeyUrl: string,
  partnerPromotionKey?: string,
) =>
  ({
    id,
    preset: {
      name: id,
      websiteUrl: `https://${id}.example`,
      apiKeyUrl,
      category: "aggregator",
      isPartner: Boolean(partnerPromotionKey),
      partnerPromotionKey,
      settingsConfig: { env: { ANTHROPIC_BASE_URL: baseUrl } },
    },
  }) as unknown as Entries[number];

const render = (props: Partial<Props>) =>
  renderHook(() =>
    useApiKeyLink({
      appId: "claude",
      category: "aggregator",
      selectedPresetId: null,
      presetEntries: [],
      formWebsiteUrl: "",
      ...props,
    }),
  ).result.current;

describe("useApiKeyLink", () => {
  const entries = [
    claudePreset(
      "hub",
      "https://hub.example.com",
      "https://hub.example/key?ref=cc",
      "hub",
    ),
    claudePreset(
      "other",
      "https://other.example.com/api",
      "https://other.example/key",
    ),
  ];

  it("edit mode finds the preset by base URL and links its apiKeyUrl", () => {
    const link = render({
      presetEntries: entries,
      formBaseUrl: "  HTTPS://hub.example.com/ ",
      formWebsiteUrl: "https://my-note.example",
    });
    expect(link.shouldShowApiKeyLink).toBe(true);
    expect(link.websiteUrl).toBe("https://hub.example/key?ref=cc");
    // 推广语跟着链接一起带出来，和添加页一致
    expect(link.isPartner).toBe(true);
    expect(link.partnerPromotionKey).toBe("hub");
  });

  it("falls back to the website field when no preset matches", () => {
    const link = render({
      presetEntries: entries,
      formBaseUrl: "https://unknown.example.com",
      formWebsiteUrl: "https://my-note.example",
    });
    expect(link.websiteUrl).toBe("https://my-note.example");
  });

  it("does not guess when presets sharing the address link to different pages", () => {
    const link = render({
      presetEntries: [
        ...entries,
        claudePreset(
          "hub-plan",
          "https://hub.example.com",
          "https://hub.example/plan",
        ),
      ],
      formBaseUrl: "https://hub.example.com",
      formWebsiteUrl: "https://my-note.example",
    });
    expect(link.websiteUrl).toBe("https://my-note.example");
  });

  it("does not guess when presets sharing the link differ in promotion", () => {
    const link = render({
      presetEntries: [
        ...entries,
        claudePreset(
          "hub-2",
          "https://hub.example.com",
          "https://hub.example/key?ref=cc",
        ),
      ],
      formBaseUrl: "https://hub.example.com",
      formWebsiteUrl: "https://my-note.example",
    });
    expect(link.websiteUrl).toBe("https://my-note.example");
    expect(link.partnerPromotionKey).toBeUndefined();
  });

  it("a selected preset still wins over the base URL lookup", () => {
    const link = render({
      presetEntries: entries,
      selectedPresetId: "other",
      formBaseUrl: "https://hub.example.com",
    });
    expect(link.websiteUrl).toBe("https://other.example/key");
  });

  it("reads each app's preset address from its own field", () => {
    const codex = render({
      appId: "codex",
      presetEntries: [
        {
          id: "codex-0",
          preset: {
            name: "Hub",
            websiteUrl: "https://hub.example",
            apiKeyUrl: "https://hub.example/codex-key",
            category: "aggregator",
            auth: {},
            config:
              'model_provider = "custom"\n\n[model_providers.custom]\nbase_url = "https://hub.example.com/v1"\n',
          },
        } as unknown as Entries[number],
      ],
      formBaseUrl: "https://hub.example.com/v1",
    });
    expect(codex.websiteUrl).toBe("https://hub.example/codex-key");

    const desktop = render({
      appId: "claude-desktop",
      presetEntries: [
        {
          id: "claude-desktop-0",
          preset: {
            name: "Hub",
            websiteUrl: "https://hub.example",
            apiKeyUrl: "https://hub.example/desktop-key",
            category: "aggregator",
            baseUrl: "https://hub.example.com",
          },
        } as unknown as Entries[number],
      ],
      formBaseUrl: "https://hub.example.com",
    });
    expect(desktop.websiteUrl).toBe("https://hub.example/desktop-key");
  });
});
