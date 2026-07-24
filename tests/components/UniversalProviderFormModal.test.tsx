import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import type { UniversalProvider } from "@/types";

vi.mock("@/hooks/useDarkMode", () => ({
  useDarkMode: () => false,
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: ReactNode;
    footer?: ReactNode;
  }) =>
    isOpen ? (
      <div>
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => null,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea aria-label="config-preview" value={value} readOnly />
  ),
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: () => null,
}));

function editingProvider(
  claudeModels: UniversalProvider["models"]["claude"],
): UniversalProvider {
  return {
    id: "universal-1",
    name: "Universal",
    providerType: "newapi",
    apps: {
      claude: true,
      codex: false,
      gemini: false,
    },
    baseUrl: "https://api.example.com",
    apiKey: "sk-test",
    models: {
      claude: claudeModels,
    },
  };
}

describe("UniversalProviderFormModal", () => {
  it("defaults Fable to Opus and saves explicitly configured Claude role models", async () => {
    const onSave = vi.fn();

    render(
      <UniversalProviderFormModal isOpen onClose={() => {}} onSave={onSave} />,
    );

    await waitFor(() => {
      expect(screen.getByLabelText("Fable")).toHaveValue("claude-opus-4-8");
    });
    expect(screen.getByLabelText("Subagent")).toHaveValue("");

    fireEvent.change(screen.getByLabelText("Fable"), {
      target: { value: "claude-fable-custom" },
    });
    fireEvent.change(screen.getByLabelText("Subagent"), {
      target: { value: "claude-subagent-custom" },
    });
    fireEvent.change(screen.getByLabelText("API 地址"), {
      target: { value: "https://api.example.com" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "sk-test" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加" }));

    expect(onSave).toHaveBeenCalledOnce();
    expect(onSave.mock.calls[0][0].models.claude).toMatchObject({
      fableModel: "claude-fable-custom",
      subagentModel: "claude-subagent-custom",
    });
  });

  it("restores Fable and Subagent values and includes them in the Claude preview", async () => {
    render(
      <UniversalProviderFormModal
        isOpen
        onClose={() => {}}
        onSave={() => {}}
        editingProvider={editingProvider({
          model: "claude-main",
          haikuModel: "claude-haiku",
          sonnetModel: "claude-sonnet",
          opusModel: "claude-opus",
          fableModel: "claude-fable",
          subagentModel: "claude-subagent",
        })}
      />,
    );

    await waitFor(() => {
      expect(screen.getByLabelText("Fable")).toHaveValue("claude-fable");
    });
    expect(screen.getByLabelText("Subagent")).toHaveValue("claude-subagent");

    const preview = JSON.parse(
      (screen.getByLabelText("config-preview") as HTMLTextAreaElement).value,
    );
    expect(preview.env).toMatchObject({
      ANTHROPIC_DEFAULT_FABLE_MODEL: "claude-fable",
      CLAUDE_CODE_SUBAGENT_MODEL: "claude-subagent",
    });
  });

  it("keeps optional role models blank for legacy providers and omits them from preview", async () => {
    render(
      <UniversalProviderFormModal
        isOpen
        onClose={() => {}}
        onSave={() => {}}
        editingProvider={editingProvider({
          model: "claude-main",
          opusModel: "claude-opus",
        })}
      />,
    );

    await waitFor(() => {
      expect(screen.getByLabelText("Fable")).toHaveValue("");
    });
    expect(screen.getByLabelText("Subagent")).toHaveValue("");

    const preview = JSON.parse(
      (screen.getByLabelText("config-preview") as HTMLTextAreaElement).value,
    );
    expect(preview.env).not.toHaveProperty("ANTHROPIC_DEFAULT_FABLE_MODEL");
    expect(preview.env).not.toHaveProperty("CLAUDE_CODE_SUBAGENT_MODEL");
  });
});
