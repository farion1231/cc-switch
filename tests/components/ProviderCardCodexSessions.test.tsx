import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { ProviderCard } from "@/components/providers/ProviderCard";

function createProvider(): Provider {
  return {
    id: "provider-a",
    name: "Provider A",
    settingsConfig: {},
  };
}

describe("ProviderCard Codex sessions action", () => {
  it("accepts a Codex sessions action handler", () => {
    const provider = createProvider();
    const element = (
      <ProviderCard
        provider={provider}
        isCurrent={false}
        appId="codex"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenCodexSessions={vi.fn()}
        isProxyRunning={false}
      />
    );

    expect(element.props.onOpenCodexSessions).toBeDefined();
  });
});
