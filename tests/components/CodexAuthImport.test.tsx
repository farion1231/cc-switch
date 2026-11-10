import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexAuthImport } from "@/components/providers/forms/CodexAuthImport";

const mocks = vi.hoisted(() => ({
  importJson: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@/lib/api/auth", () => ({
  authImportCodexJson: mocks.importJson,
  CODEX_OAUTH_DUPLICATE_ACCOUNT_ERROR: "codex_oauth_duplicate_account",
}));
vi.mock("@/lib/toast", () => ({
  toast: { success: mocks.success, error: mocks.error },
}));

describe("CodexAuthImport", () => {
  beforeEach(() => vi.clearAllMocks());
  function file(text = '{"tokens":{}}') {
    const value = new File([text], "auth.json", { type: "application/json" });
    Object.defineProperty(value, "text", {
      value: vi.fn().mockResolvedValue(text),
    });
    return value;
  }
  it("imports the selected file, refreshes accounts, and allows selecting the same file again", async () => {
    const refresh = vi.fn().mockResolvedValue(undefined);
    mocks.importJson.mockResolvedValue({ login: "alice@example.com" });
    const { container } = render(
      <CodexAuthImport disabled={false} onImported={refresh} />,
    );
    const input = container.querySelector(
      'input[type="file"]',
    ) as HTMLInputElement;
    const value = file();
    await userEvent.upload(input, value);
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
    expect(mocks.importJson).toHaveBeenCalledWith('{"tokens":{}}');
    expect(mocks.success).toHaveBeenCalled();
    expect(input.value).toBe("");
    await userEvent.upload(input, value);
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(2));
  });
  it("shows duplicate errors without claiming success", async () => {
    mocks.importJson.mockRejectedValue("codex_oauth_duplicate_account");
    const refresh = vi.fn();
    const { container } = render(
      <CodexAuthImport disabled={false} onImported={refresh} />,
    );
    await userEvent.upload(container.querySelector("input")!, file());
    await waitFor(() => expect(mocks.error).toHaveBeenCalled());
    expect(refresh).not.toHaveBeenCalled();
    expect(mocks.success).not.toHaveBeenCalled();
  });
  it("rejects large files before reading credentials", async () => {
    const { container } = render(
      <CodexAuthImport disabled={false} onImported={vi.fn()} />,
    );
    const value = file();
    Object.defineProperty(value, "size", { value: 1024 * 1024 + 1 });
    await userEvent.upload(container.querySelector("input")!, value);
    expect(mocks.error).toHaveBeenCalled();
    expect(value.text).not.toHaveBeenCalled();
    expect(mocks.importJson).not.toHaveBeenCalled();
  });
  it("disables importing while another login is active", () => {
    render(<CodexAuthImport disabled onImported={vi.fn()} />);
    expect(screen.getByRole("button")).toBeDisabled();
  });
  it("imports pasted JSON through the shared API and clears it after success", async () => {
    const user = userEvent.setup();
    const refresh = vi.fn().mockResolvedValue(undefined);
    mocks.importJson.mockResolvedValue({ login: "alice@example.com" });
    render(<CodexAuthImport disabled={false} onImported={refresh} />);
    await user.click(
      screen.getByRole("button", { name: "codexOauth.importAuthJson" }),
    );
    const submit = screen.getByRole("button", {
      name: "codexOauth.importPasted",
    });
    expect(submit).toBeDisabled();
    await user.click(screen.getByRole("textbox"));
    const text = '{"tokens":{"refresh_token":"test-paste-only"}}';
    await user.paste(text);
    await user.click(submit);
    await waitFor(() => expect(refresh).toHaveBeenCalledOnce());
    expect(mocks.importJson).toHaveBeenCalledWith(text);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "codexOauth.importAuthJson" }),
    );
    expect(screen.getByRole("textbox")).toHaveValue("");
  });
  it("clears pasted credentials when cancelled without importing", async () => {
    const user = userEvent.setup();
    render(<CodexAuthImport disabled={false} onImported={vi.fn()} />);
    await user.click(
      screen.getByRole("button", { name: "codexOauth.importAuthJson" }),
    );
    await user.click(screen.getByRole("textbox"));
    await user.paste("test-private-content");
    await user.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(mocks.importJson).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", { name: "codexOauth.importAuthJson" }),
    );
    expect(screen.getByRole("textbox")).toHaveValue("");
  });
  it("keeps failed pasted input available for correction", async () => {
    const user = userEvent.setup();
    mocks.importJson.mockRejectedValue("invalid JSON");
    render(<CodexAuthImport disabled={false} onImported={vi.fn()} />);
    await user.click(
      screen.getByRole("button", { name: "codexOauth.importAuthJson" }),
    );
    await user.click(screen.getByRole("textbox"));
    await user.paste("{invalid");
    await user.click(
      screen.getByRole("button", { name: "codexOauth.importPasted" }),
    );
    await waitFor(() => expect(screen.getByRole("alert")).toBeVisible());
    expect(screen.getByRole("textbox")).toHaveValue("{invalid");
    expect(screen.getByRole("textbox")).toHaveAttribute("aria-invalid", "true");
  });
});
