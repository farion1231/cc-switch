import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { AuxiliaryQueueManager } from "@/components/proxy/AuxiliaryQueueManager";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const setConfigMock = vi.fn().mockResolvedValue(undefined);
const addMock = vi.fn().mockResolvedValue(undefined);
const removeMock = vi.fn().mockResolvedValue(undefined);
const reorderMock = vi.fn().mockResolvedValue(undefined);
const setModelMock = vi.fn().mockResolvedValue(undefined);

let auxiliaryConfig = { enabled: false };
let auxiliaryQueue: Array<{
  providerId: string;
  providerName: string;
  model?: string;
}> = [];

vi.mock("@/lib/query/auxiliary", () => ({
  useAuxiliaryConfig: () => ({ data: auxiliaryConfig }),
  useSetAuxiliaryConfig: () => ({
    mutateAsync: setConfigMock,
    isPending: false,
  }),
  useAuxiliaryQueue: () => ({
    data: auxiliaryQueue,
    isLoading: false,
    error: null,
  }),
  useAvailableProvidersForAuxiliary: () => ({
    data: [{ id: "fast", name: "Fast Provider" }],
    isLoading: false,
  }),
  useAddToAuxiliaryQueue: () => ({ mutateAsync: addMock, isPending: false }),
  useRemoveFromAuxiliaryQueue: () => ({
    mutateAsync: removeMock,
    isPending: false,
  }),
  useReorderAuxiliaryQueue: () => ({
    mutateAsync: reorderMock,
    isPending: false,
  }),
  useSetAuxiliaryModel: () => ({
    mutateAsync: setModelMock,
    isPending: false,
  }),
}));

describe("AuxiliaryQueueManager", () => {
  beforeEach(() => {
    setConfigMock.mockClear();
    addMock.mockClear();
    removeMock.mockClear();
    reorderMock.mockClear();
    setModelMock.mockClear();
    auxiliaryConfig = { enabled: false };
    auxiliaryQueue = [];
  });

  it("explains the fallback behaviour when the queue is empty", () => {
    render(<AuxiliaryQueueManager appType="claude" />);
    expect(screen.getByText("proxy.auxiliaryQueue.empty")).toBeInTheDocument();
  });

  it("persists the master switch when it is toggled on", async () => {
    render(<AuxiliaryQueueManager appType="claude" />);

    fireEvent.click(
      screen.getByRole("switch", { name: "proxy.auxiliary.enable" }),
    );

    await waitFor(() =>
      expect(setConfigMock).toHaveBeenCalledWith({
        appType: "claude",
        config: { enabled: true },
      }),
    );
  });

  it("offers no thinking switch — the proxy never rewrites thinking", () => {
    // 开不开思考由 Claude Code 的报文自己决定，面板上不该再有这个开关
    auxiliaryConfig = { enabled: true };
    render(<AuxiliaryQueueManager appType="claude" />);

    expect(screen.getAllByRole("switch")).toHaveLength(1);
    expect(
      screen.queryByRole("switch", {
        name: "proxy.auxiliary.forceThinkingOff",
      }),
    ).not.toBeInTheDocument();
  });

  it("renders queued providers in order with a position badge", () => {
    auxiliaryQueue = [
      { providerId: "fast", providerName: "Fast Provider" },
      { providerId: "backup", providerName: "Backup Provider" },
    ];
    render(<AuxiliaryQueueManager appType="claude" />);

    expect(screen.getByText("Fast Provider")).toBeInTheDocument();
    expect(screen.getByText("Backup Provider")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("removes a provider from the queue", async () => {
    auxiliaryQueue = [{ providerId: "fast", providerName: "Fast Provider" }];
    render(<AuxiliaryQueueManager appType="claude" />);

    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));

    await waitFor(() =>
      expect(removeMock).toHaveBeenCalledWith({
        appType: "claude",
        providerId: "fast",
      }),
    );
  });

  it("offers a drag handle per queued provider", () => {
    auxiliaryQueue = [
      { providerId: "fast", providerName: "Fast Provider" },
      { providerId: "backup", providerName: "Backup Provider" },
    ];
    render(<AuxiliaryQueueManager appType="claude" />);

    expect(
      screen.getAllByRole("button", {
        name: "proxy.auxiliaryQueue.dragHandle",
      }),
    ).toHaveLength(2);
  });

  it("shows the stored model override in the input", () => {
    auxiliaryQueue = [
      {
        providerId: "fast",
        providerName: "Fast Provider",
        model: "glm-4-flash",
      },
    ];
    render(<AuxiliaryQueueManager appType="claude" />);

    expect(
      screen.getByRole("textbox", { name: "proxy.auxiliaryQueue.modelLabel" }),
    ).toHaveValue("glm-4-flash");
  });

  it("saves the model override on blur", async () => {
    auxiliaryQueue = [{ providerId: "fast", providerName: "Fast Provider" }];
    render(<AuxiliaryQueueManager appType="claude" />);

    const input = screen.getByRole("textbox", {
      name: "proxy.auxiliaryQueue.modelLabel",
    });
    fireEvent.change(input, { target: { value: "  glm-4-flash  " } });
    fireEvent.blur(input);

    await waitFor(() =>
      expect(setModelMock).toHaveBeenCalledWith({
        appType: "claude",
        providerId: "fast",
        model: "glm-4-flash",
      }),
    );
  });

  it("clears the override when the input is emptied", async () => {
    auxiliaryQueue = [
      {
        providerId: "fast",
        providerName: "Fast Provider",
        model: "glm-4-flash",
      },
    ];
    render(<AuxiliaryQueueManager appType="claude" />);

    const input = screen.getByRole("textbox", {
      name: "proxy.auxiliaryQueue.modelLabel",
    });
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.blur(input);

    await waitFor(() =>
      expect(setModelMock).toHaveBeenCalledWith({
        appType: "claude",
        providerId: "fast",
        model: null,
      }),
    );
  });

  it("does not write when the model is unchanged", () => {
    auxiliaryQueue = [
      {
        providerId: "fast",
        providerName: "Fast Provider",
        model: "glm-4-flash",
      },
    ];
    render(<AuxiliaryQueueManager appType="claude" />);

    // 单纯聚焦再离开不该产生一次写入
    fireEvent.blur(
      screen.getByRole("textbox", {
        name: "proxy.auxiliaryQueue.modelLabel",
      }),
    );

    expect(setModelMock).not.toHaveBeenCalled();
  });
});
