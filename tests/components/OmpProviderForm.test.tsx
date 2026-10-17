import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OmpProviderForm } from "@/components/providers/forms/OmpProviderForm";
import { renderWithQueryClient as render } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

describe("OmpProviderForm", () => {
  it("creates a native omp node without Pi-only fields", async () => {
    const onSubmit = vi.fn();
    const { container } = render(
      <OmpProviderForm
        appId="omp"
        submitLabel="Save omp"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "New omp" },
    });
    fireEvent.change(container.querySelector("#pi-provider-key")!, {
      target: { value: "cc-switch-test" },
    });
    const config = {
      baseUrl: "https://example.com/v1",
      apiKey: "LITERAL_ENV_NAME",
      api: "openai-completions",
      models: [
        {
          id: "model-a",
          name: "Model A",
          reasoning: true,
          input: ["text"],
          contextWindow: 64000,
          maxTokens: 8000,
        },
      ],
    };
    fireEvent.change(container.querySelector("#pi-settings-config")!, {
      target: { value: JSON.stringify(config) },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save omp" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit.mock.calls[0][0].name).toBe("New omp");
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toEqual(
      config,
    );
  });

  it.each([undefined, "Test"])(
    "preserves native fields and provider name %s on edit",
    async (nativeName) => {
      const onSubmit = vi.fn();
      const config = {
        ...(nativeName === undefined ? {} : { name: nativeName }),
        baseUrl: "https://example.com/v1",
        apiKey: "LITERAL_ENV_NAME",
        api: "openai-completions",
        auth: "apiKey",
        authHeader: true,
        modelOverrides: { builtin: { contextWindow: 64000 } },
        models: [
          {
            id: "model-a",
            reasoning: true,
            thinking: { mode: "effort", efforts: ["low", "high"] },
            compat: { supportsStore: false },
          },
        ],
      };
      render(
        <OmpProviderForm
          appId="omp"
          providerId="cc-switch-test"
          initialData={{ name: "Test", settingsConfig: config }}
          submitLabel="Save omp"
          onSubmit={onSubmit}
          onCancel={() => {}}
        />,
      );
      expect(
        screen.queryByText("pi.form.customizeThinkingLevels"),
      ).not.toBeInTheDocument();
      fireEvent.change(screen.getByLabelText("provider.name"), {
        target: { value: "Edited" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Save omp" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
      const saved = onSubmit.mock.calls[0][0];
      expect(saved.providerKey).toBe("cc-switch-test");
      expect(saved.name).toBe("Edited");
      expect(JSON.parse(saved.settingsConfig)).toEqual(config);
    },
  );
});
