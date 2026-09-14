import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { McodeSettings } from "@/components/settings/McodeSettings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
const invokeMock = vi.mocked(invoke);

describe("MCode credential settings", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({ source: "token_plan", hasApiKey: false });
  });

  it("reads status without writing credentials and disables actions that need a key", async () => {
    render(<McodeSettings />);
    await screen.findByText(/mcode.noKey/);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("manage_mcode", {
      action: "status",
      apiKey: null,
    });
    expect(screen.getByRole("button", { name: "mcode.test" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "mcode.useApiKey" }),
    ).toBeDisabled();
  });

  it("masks and clears a saved key, then switches and tests through MCode", async () => {
    render(<McodeSettings />);
    await screen.findByText(/mcode.noKey/);
    const input = screen.getByLabelText("MiniMax API Key");
    expect(input).toHaveAttribute("type", "password");
    fireEvent.change(input, { target: { value: "test-key" } });
    invokeMock.mockResolvedValue({
      source: "minimax_api_key",
      hasApiKey: true,
    });
    fireEvent.click(screen.getByRole("button", { name: "mcode.saveAndUse" }));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(invokeMock).toHaveBeenLastCalledWith("manage_mcode", {
      action: "saveApiKey",
      apiKey: "test-key",
    });
    expect(screen.getByRole("status")).toHaveTextContent("mcode.saved");
    fireEvent.click(screen.getByRole("button", { name: "mcode.test" }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("mcode.testPassed"),
    );
    expect(invokeMock).toHaveBeenLastCalledWith("manage_mcode", {
      action: "test",
      apiKey: null,
    });
    invokeMock.mockResolvedValue({ source: "token_plan", hasApiKey: true });
    fireEvent.click(screen.getByRole("button", { name: "mcode.useTokenPlan" }));
    await screen.findByText(/Token Plan/);
    expect(invokeMock).toHaveBeenLastCalledWith("manage_mcode", {
      action: "useTokenPlan",
      apiKey: null,
    });
  });

  it("shows a failed connectivity check as an error, never a success", async () => {
    invokeMock.mockResolvedValue({
      source: "minimax_api_key",
      hasApiKey: true,
    });
    render(<McodeSettings />);
    await screen.findByText(/mcode.keySaved/);
    invokeMock.mockRejectedValue("MiniMax API key test failed");
    fireEvent.click(screen.getByRole("button", { name: "mcode.test" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "MiniMax API key test failed",
    );
    expect(screen.getByRole("status")).not.toHaveTextContent(
      "mcode.testPassed",
    );
  });

  it("allows retry after installation was unavailable", async () => {
    invokeMock.mockRejectedValueOnce("mcode is not installed");
    render(<McodeSettings />);
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "common.refresh" }));
    await screen.findByText(/mcode.noKey/);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
