import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DshProvidersPage } from "@/components/dsh/DshProvidersPage";
import type {
  DshCredentialWrite,
  DshCustomInput,
  DshSnapshot,
} from "@/lib/api/dsh";

const mocks = vi.hoisted(() => ({
  createCustom: vi.fn(),
  updateCustom: vi.fn(),
  setCredential: vi.fn(),
  refresh: vi.fn(),
  toastWarning: vi.fn(),
  toastSuccess: vi.fn(),
}));

const snapshot: DshSnapshot = {
  home: "/mock/dsh",
  settingsPath: "/mock/dsh/settings.yaml",
  credentialsPath: "/mock/dsh/.env",
  settingsRevision: "settings-1",
  credentialsRevision: "credentials-1",
  readOnly: false,
  providers: [],
  defaultModel: null,
  protocols: ["openai-completions"],
};

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    warning: mocks.toastWarning,
    success: mocks.toastSuccess,
  },
}));

vi.mock("@/lib/query/dsh", () => ({
  useDshSnapshot: () => ({
    data: snapshot,
    isLoading: false,
    isFetching: false,
  }),
  useDshActions: () => ({
    refresh: mocks.refresh,
    upsertNative: vi.fn(),
    resetNative: vi.fn(),
    createCustom: mocks.createCustom,
    updateCustom: mocks.updateCustom,
    removeCustom: vi.fn(),
    setDefaultModel: vi.fn(),
    setCredential: mocks.setCredential,
    unsetCredential: vi.fn(),
    discoverModels: vi.fn(),
    openHome: vi.fn(),
  }),
}));

vi.mock("@/components/dsh/DshDefaultModelPicker", () => ({
  DshDefaultModelPicker: () => null,
}));

vi.mock("@/components/dsh/DshProviderDialog", () => ({
  DshProviderDialog: ({
    open,
    onClose,
    onSaveCustom,
  }: {
    open: boolean;
    onClose: () => void;
    onSaveCustom: (
      input: DshCustomInput,
      key: DshCredentialWrite,
    ) => Promise<void>;
  }) =>
    open ? (
      <button
        type="button"
        onClick={async () => {
          await onSaveCustom(
            {
              route: "acme",
              api: "openai-completions",
              baseURL: "https://example.test",
              models: [{ id: "chat" }],
            },
            {
              ref: "ACME_API_KEY",
              value: "secret",
              expectedRevision: "credentials-1",
            },
          );
          onClose();
        }}
      >
        save-custom
      </button>
    ) : null,
}));

describe("DshProvidersPage", () => {
  beforeEach(() => {
    mocks.createCustom.mockResolvedValue(snapshot);
    mocks.updateCustom.mockResolvedValue(snapshot);
    mocks.setCredential.mockRejectedValue(
      JSON.stringify({ code: "credentials-conflict", message: "changed" }),
    );
    mocks.refresh.mockResolvedValue(snapshot);
  });

  it("does not retry provider creation when the staged key write fails", async () => {
    render(<DshProvidersPage />);

    fireEvent.click(
      screen.getByRole("button", { name: "dsh.actions.addProvider" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-custom" }));

    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "save-custom" })).toBeNull(),
    );
    expect(mocks.createCustom).toHaveBeenCalledTimes(1);
    expect(mocks.setCredential).toHaveBeenCalledWith({
      ref: "ACME_API_KEY",
      value: "secret",
      expectedRevision: "credentials-1",
    });
    expect(mocks.toastWarning).toHaveBeenCalledWith(
      "dsh.messages.profileSavedKeyFailed",
      expect.objectContaining({ description: "dsh.errors.conflict" }),
    );
  });
});
