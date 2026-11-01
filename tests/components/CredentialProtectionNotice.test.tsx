import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { CredentialProtectionNotice } from "@/components/providers/CredentialProtectionNotice";
import ApiKeyInput from "@/components/providers/forms/ApiKeyInput";
import { CredentialEditProtectionContext } from "@/contexts/CredentialEditProtectionContext";
import { providersApi } from "@/lib/api";
import { toast } from "@/lib/toast";

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({
    isOpen,
    onConfirm,
    onCancel,
    pending,
  }: {
    isOpen: boolean;
    onConfirm: () => void;
    onCancel: () => void;
    pending: boolean;
  }) =>
    isOpen ? (
      <div>
        <button disabled={pending} onClick={onConfirm}>
          confirm-remove
        </button>
        <button disabled={pending} onClick={onCancel}>
          cancel-remove
        </button>
      </div>
    ) : null,
}));

afterEach(() => vi.restoreAllMocks());

it("makes a protected key read-only without changing reveal behavior", () => {
  const onChange = vi.fn();
  render(
    <CredentialEditProtectionContext.Provider value={true}>
      <ApiKeyInput label="Key" value="public-test-key" onChange={onChange} />
    </CredentialEditProtectionContext.Provider>,
  );
  const key = screen.getByLabelText("Key");
  expect(key).toHaveAttribute("readonly");
  fireEvent.change(key, { target: { value: "wrong-key" } });
  expect(onChange).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "apiKeyInput.show" }));
  expect(key).toHaveAttribute("type", "text");
});

it("keeps ordinary keys editable", () => {
  const onChange = vi.fn();
  render(
    <ApiKeyInput label="Key" value="public-test-key" onChange={onChange} />,
  );
  const key = screen.getByLabelText("Key");
  expect(key).not.toHaveAttribute("readonly");
  fireEvent.change(key, { target: { value: "replacement" } });
  expect(onChange).toHaveBeenCalledWith("replacement");
});

describe("removing credential editing protection", () => {
  beforeEach(() => {
    vi.spyOn(providersApi, "removeCredentialProtection").mockResolvedValue(
      undefined,
    );
    vi.spyOn(toast, "error").mockReturnValue("test-toast");
  });
  const setup = () => {
    const onRemoved = vi.fn();
    const onPendingChange = vi.fn();
    const result = render(
      <CredentialProtectionNotice
        appId="claude"
        providerId="one"
        onRemoved={onRemoved}
        onPendingChange={onPendingChange}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "credentialProtection.remove" }),
    );
    return { ...result, onRemoved, onPendingChange };
  };

  it("does not change protection when confirmation is canceled", () => {
    const { onRemoved } = setup();
    fireEvent.click(screen.getByText("cancel-remove"));
    expect(providersApi.removeCredentialProtection).not.toHaveBeenCalled();
    expect(onRemoved).not.toHaveBeenCalled();
  });

  it("targets the selected app/provider only after confirmation", async () => {
    const { onRemoved, onPendingChange } = setup();
    fireEvent.click(screen.getByText("confirm-remove"));
    await waitFor(() => expect(onRemoved).toHaveBeenCalledTimes(1));
    expect(providersApi.removeCredentialProtection).toHaveBeenCalledWith(
      "one",
      "claude",
    );
    expect(onPendingChange).toHaveBeenCalledWith(true);
  });

  it("keeps the editor open on failure", async () => {
    vi.mocked(providersApi.removeCredentialProtection).mockRejectedValue(
      new Error("failed"),
    );
    const { onRemoved } = setup();
    fireEvent.click(screen.getByText("confirm-remove"));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "credentialProtection.removeFailed",
      ),
    );
    expect(onRemoved).not.toHaveBeenCalled();
  });

  it("does not close a different editor after the original notice unmounts", async () => {
    let resolve!: () => void;
    vi.mocked(providersApi.removeCredentialProtection).mockReturnValue(
      new Promise<void>((done) => {
        resolve = done;
      }),
    );
    const { onRemoved, unmount } = setup();
    fireEvent.click(screen.getByText("confirm-remove"));
    unmount();
    await act(async () => resolve());
    expect(onRemoved).not.toHaveBeenCalled();
  });
});
