import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderCredentialConflict } from "@/components/providers/ProviderCredentialConflict";

const mocks = vi.hoisted(() => ({
  importLiveCredentials: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
}));

vi.mock("@/lib/api/providerSecurity", () => ({
  providerSecurityApi: {
    importLiveCredentials: mocks.importLiveCredentials,
  },
}));

vi.mock("sonner", () => ({
  toast: {
    error: mocks.toastError,
    success: mocks.toastSuccess,
  },
}));

describe("ProviderCredentialConflict", () => {
  beforeEach(() => {
    mocks.importLiveCredentials.mockReset();
    mocks.toastError.mockReset();
    mocks.toastSuccess.mockReset();
  });

  it("keeps a stale import on the conflict path", async () => {
    mocks.importLiveCredentials.mockResolvedValue({
      kind: "conflict",
      currentRevision: 2,
      diff: [],
    });
    const onImported = vi.fn();

    render(
      <ProviderCredentialConflict
        appId="codex"
        providerId="provider-1"
        revision={1}
        conflicts={[
          {
            field: "apiKey",
            storedMasked: "sk-db***",
            liveMasked: "sk-live***",
            storedFingerprint: "db-fingerprint",
            liveFingerprint: "live-fingerprint",
          },
        ]}
        onImported={onImported}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "从 Live 导入" }));
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "确认导入" }));

    await waitFor(() => {
      expect(mocks.importLiveCredentials).toHaveBeenCalledWith({
        appId: "codex",
        providerId: "provider-1",
        expectedRevision: 1,
        fields: ["apiKey"],
      });
    });
    expect(mocks.toastError).toHaveBeenCalledWith(
      "供应商已被其他操作更新，请重新加载后再导入",
    );
    expect(mocks.toastSuccess).not.toHaveBeenCalled();
    expect(onImported).not.toHaveBeenCalled();
  });
});
