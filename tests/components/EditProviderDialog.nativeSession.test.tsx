import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import type { Provider } from "@/types";
import { renderWithQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({ default: () => null }));

const provider: Provider = {
  id: "example-provider",
  name: "Example provider",
  settingsConfig: {
    kind: "custom",
    api: "openai-completions",
    options: { baseURL: "https://example.test/v1", apiKey: "test-key" },
    models: { "example-model": { name: "Example model" } },
  },
};

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe.each(["dsh", "mcode"] as const)("%s editor sessions", (appId) => {
  it.each([true, false])(
    "can save after cancelling a pending edit and reopening (wait for exit: %s)",
    async (waitForExit) => {
      const previous = deferred();
      const current = deferred();
      const onSubmit = vi
        .fn()
        .mockReturnValueOnce(previous.promise)
        .mockReturnValueOnce(current.promise);
      const props = { appId, provider, onSubmit, onOpenChange: vi.fn() };
      const { rerender } = renderWithQueryClient(
        <EditProviderDialog open {...props} />,
      );

      fireEvent.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
      expect(
        screen.getByRole("button", { name: "common.save" }),
      ).toBeDisabled();
      fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
      expect(props.onOpenChange).toHaveBeenCalledWith(false);
      rerender(<EditProviderDialog open={false} {...props} />);
      if (waitForExit) {
        await waitFor(() =>
          expect(
            screen.queryByRole("button", { name: "common.save" }),
          ).not.toBeInTheDocument(),
        );
      }
      props.onOpenChange.mockClear();
      rerender(<EditProviderDialog open {...props} />);
      const save = await screen.findByRole("button", { name: "common.save" });
      expect(save).toBeEnabled();
      expect(screen.getByDisplayValue("Example model")).toBeEnabled();
      fireEvent.change(screen.getByDisplayValue("Example model"), {
        target: { value: "Edited model" },
      });
      fireEvent.click(save);
      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(2));

      await act(async () => previous.resolve());
      expect(props.onOpenChange).not.toHaveBeenCalled();
      expect(save).toBeDisabled();
      expect(screen.getByDisplayValue("Edited model")).toBeDisabled();

      await act(async () => current.resolve());
      expect(props.onOpenChange).toHaveBeenCalledTimes(1);
      expect(props.onOpenChange).toHaveBeenCalledWith(false);
      expect(save).toBeEnabled();
      expect(
        onSubmit.mock.calls[1][0].provider.settingsConfig.models[
          "example-model"
        ].name,
      ).toBe("Edited model");
    },
  );

  it("keeps a different provider editable when the previous save completes", async () => {
    const previous = deferred();
    const props = {
      appId,
      provider,
      onSubmit: vi
        .fn()
        .mockReturnValueOnce(previous.promise)
        .mockResolvedValue(undefined),
      onOpenChange: vi.fn(),
    };
    const { rerender } = renderWithQueryClient(
      <EditProviderDialog open {...props} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(props.onSubmit).toHaveBeenCalledTimes(1));
    rerender(
      <EditProviderDialog
        open
        {...props}
        provider={{ ...provider, id: "other-provider", name: "Other provider" }}
      />,
    );
    const save = screen.getByRole("button", { name: "common.save" });
    expect(save).toBeEnabled();
    expect(screen.getByDisplayValue("Other provider")).toBeEnabled();
    await act(async () => previous.resolve());
    expect(props.onOpenChange).not.toHaveBeenCalled();
    await act(async () => fireEvent.click(save));
    expect(props.onSubmit).toHaveBeenCalledTimes(2);
    expect(props.onSubmit.mock.calls[1][0].provider).toMatchObject({
      id: "other-provider",
      name: "Other provider",
    });
    expect(props.onOpenChange).toHaveBeenCalledWith(false);
  });
});
