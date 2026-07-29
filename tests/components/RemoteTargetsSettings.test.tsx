import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  upsertTarget,
  deleteTarget,
  setActiveTarget,
  testTarget,
  discoverTargets,
} = vi.hoisted(() => ({
  upsertTarget: vi.fn(),
  deleteTarget: vi.fn(),
  setActiveTarget: vi.fn(),
  testTarget: vi.fn(),
  discoverTargets: vi.fn(),
}));

// 此处只替换运行时和 Tauri 边界，表单交互仍走真实组件，避免测试实现细节。
vi.mock("@/contexts/RuntimeTargetContext", () => ({
  useRuntimeTarget: () => ({
    snapshot: { status: "local", generation: 0 },
    targets: [{ id: "prod", name: "Production", hostAlias: "prod-api" }],
    upsertTarget,
    deleteTarget,
    setActiveTarget,
  }),
}));

vi.mock("@/lib/api/remote", () => ({
  remoteApi: { testTarget, discoverTargets },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string; name?: string }) =>
      options?.defaultValue?.replace("{{name}}", options?.name ?? "") ?? key,
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, onConfirm }: any) =>
    isOpen ? (
      <button onClick={() => onConfirm(false)}>confirm-delete</button>
    ) : null,
}));

import { RemoteTargetsSettings } from "@/components/remote/RemoteTargetsSettings";

describe("RemoteTargetsSettings", () => {
  beforeEach(() => {
    upsertTarget.mockReset().mockResolvedValue(undefined);
    deleteTarget.mockReset().mockResolvedValue(undefined);
    setActiveTarget.mockReset().mockResolvedValue(undefined);
    testTarget
      .mockReset()
      .mockResolvedValue({ os: "linux", architecture: "x86_64" });
    discoverTargets.mockReset().mockResolvedValue([
      {
        name: "staging-box",
        hostAlias: "staging-box",
        hostname: "10.0.0.8",
        username: "deploy",
        port: 2222,
        identityFile: "~/.ssh/staging",
      },
    ]);
  });

  it("provides an explicit close button for the server dialog", async () => {
    const user = userEvent.setup();
    render(<RemoteTargetsSettings />);

    await user.click(screen.getByRole("button", { name: "Add server" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Close" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("loads SSH config servers by default and imports their resolved fields", async () => {
    const user = userEvent.setup();
    render(<RemoteTargetsSettings />);

    expect(discoverTargets).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Add server" }));
    await user.click(
      await screen.findByRole("button", { name: "Use staging-box" }),
    );

    expect(screen.getByLabelText("Name")).toHaveValue("staging-box");
    expect(screen.getByLabelText("SSH Host")).toHaveValue("staging-box");
    expect(screen.getByLabelText("Username")).toHaveValue("deploy");
    expect(screen.getByLabelText("Port")).toHaveValue(2222);
    expect(screen.getByLabelText("Private key")).toHaveValue("~/.ssh/staging");
  });

  it("adds a server with basic and advanced SSH fields", async () => {
    const user = userEvent.setup();
    render(<RemoteTargetsSettings />);

    await user.click(screen.getByRole("button", { name: "Add server" }));
    await user.type(screen.getByLabelText("Name"), "Staging");
    await user.type(screen.getByLabelText("SSH Host"), "staging-api");
    await user.click(screen.getByRole("button", { name: "Advanced options" }));
    await user.type(screen.getByLabelText("Username"), "deploy");
    await user.type(screen.getByLabelText("Port"), "2222");
    await user.type(screen.getByLabelText("Private key"), "~/.ssh/staging");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(upsertTarget).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Staging",
        hostAlias: "staging-api",
        username: "deploy",
        port: 2222,
        identityFile: "~/.ssh/staging",
      }),
    );
  });

  it("tests an edited configuration before saving", async () => {
    const user = userEvent.setup();
    render(<RemoteTargetsSettings />);

    await user.click(screen.getByRole("button", { name: "Edit Production" }));
    await user.click(screen.getByRole("button", { name: "Test connection" }));

    expect(testTarget).toHaveBeenCalledWith(
      expect.objectContaining({ id: "prod", hostAlias: "prod-api" }),
    );
  });

  it("connects and deletes a saved server through explicit actions", async () => {
    const user = userEvent.setup();
    render(<RemoteTargetsSettings />);

    await user.click(
      screen.getByRole("button", { name: "Connect Production" }),
    );
    expect(setActiveTarget).toHaveBeenCalledWith("prod");

    await user.click(screen.getByRole("button", { name: "Delete Production" }));
    await user.click(screen.getByText("confirm-delete"));
    expect(deleteTarget).toHaveBeenCalledWith("prod");
  });
});
