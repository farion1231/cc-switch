import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  limits: {
    memory: 2200,
    user: 1375,
    memoryEnabled: true,
    userEnabled: true,
  },
  save: vi.fn(),
  toggle: vi.fn(),
  openWebUI: vi.fn(),
  toastSuccess: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: { success: mocks.toastSuccess, error: vi.fn() },
}));

vi.mock("@/hooks/useHermes", () => ({
  useHermesMemory: (kind: string) => ({
    data: kind === "memory" ? "agent notes" : "user profile",
    isLoading: false,
    isError: false,
  }),
  useHermesMemoryLimits: () => ({ data: mocks.limits }),
  useHermesConfigDir: () => ({ data: "/Users/octocat/.hermes" }),
  useOpenHermesWebUI: () => mocks.openWebUI,
  useSaveHermesMemory: () => ({ mutateAsync: mocks.save, isPending: false }),
  useToggleHermesMemoryEnabled: () => ({
    mutate: mocks.toggle,
    isPending: false,
  }),
}));

import HermesMemoryPanel, {
  HermesMemorySaveButton,
} from "@/components/hermes/HermesMemoryPanel";

function renderPage(onOpenWebUI?: (path?: string) => void) {
  return render(
    <>
      <HermesMemorySaveButton />
      <HermesMemoryPanel onOpenWebUI={onOpenWebUI} />
    </>,
  );
}

const editors = () =>
  screen.getAllByLabelText("hermes.memory.editorLabel", {
    selector: "textarea",
  });

describe("HermesMemoryPanel", () => {
  beforeEach(() => {
    mocks.limits = {
      memory: 2200,
      user: 1375,
      memoryEnabled: true,
      userEnabled: true,
    };
    mocks.save.mockReset().mockResolvedValue(undefined);
    mocks.toggle.mockReset();
    mocks.openWebUI.mockReset();
    mocks.toastSuccess.mockReset();
  });

  it("switches memory files with secondary underline tabs and keeps unsaved edits", () => {
    renderPage();

    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      "hermes.memory.agentNameMEMORY.md",
      "hermes.memory.userNameUSER.md",
    ]);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("group")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("img", { name: "hermes.memory.unsaved" }),
    ).not.toBeInTheDocument();

    fireEvent.change(editors()[0], { target: { value: "draft" } });
    // 有未保存修改的那份页签上出现小圆点
    expect(tabs[0]).toContainElement(
      screen.getByRole("img", { name: "hermes.memory.unsaved" }),
    );

    fireEvent.click(tabs[1]);
    expect(tabs[1]).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveAttribute(
      "aria-labelledby",
      "hermes-memory-user",
    );

    fireEvent.click(tabs[0]);
    expect(editors()[0]).toHaveValue("draft");
  });

  it("saves the active file from the header button and clears the dot", async () => {
    renderPage();

    const save = screen.getByRole("button", { name: "common.save" });
    expect(save).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(save);
    expect(mocks.save).not.toHaveBeenCalled();

    fireEvent.click(screen.getAllByRole("tab")[1]);
    fireEvent.change(editors()[1], { target: { value: "new profile" } });
    const enabledSave = screen.getByRole("button", { name: "common.save" });
    expect(enabledSave).not.toHaveAttribute("aria-disabled");
    fireEvent.click(enabledSave);

    await waitFor(() =>
      expect(mocks.save).toHaveBeenCalledWith({
        kind: "user",
        content: "new profile",
      }),
    );
    // 「下次启动或新建会话时生效」是后果，写在保存成功的 toast 里
    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "hermes.memory.saveSuccess",
      ),
    );
    expect(
      screen.queryByRole("img", { name: "hermes.memory.unsaved" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "common.save" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("shows the file path when enabled and the skip warning when disabled", () => {
    mocks.limits = { ...mocks.limits, userEnabled: false };
    renderPage();

    const [agentPanel, userPanel] = screen.getAllByRole("tabpanel", {
      hidden: true,
    });
    expect(agentPanel).toHaveTextContent("~/.hermes/memories/MEMORY.md");
    expect(userPanel).toHaveTextContent("hermes.memory.disabledHint");
    expect(userPanel).not.toHaveTextContent("USER.md");

    const switches = screen.getAllByRole("switch", { hidden: true });
    fireEvent.click(switches[1]);
    expect(mocks.toggle).toHaveBeenCalledWith(
      { kind: "user", enabled: true },
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
    mocks.toggle.mock.calls[0][1].onSuccess();
    expect(mocks.toastSuccess).toHaveBeenCalledWith("hermes.memory.toggledOn");
  });

  it("writes the budget as remaining characters and flags overflow", () => {
    mocks.limits = { ...mocks.limits, memory: 10 };
    renderPage();

    const meter = screen.getAllByRole("meter")[0];
    expect(meter).toHaveAttribute("aria-valuenow", "0");
    expect(screen.getAllByRole("tabpanel")[0]).toHaveTextContent(
      "hermes.memory.overLimitBy",
    );

    fireEvent.change(editors()[0], { target: { value: "short" } });
    expect(screen.getAllByRole("meter")[0]).toHaveAttribute(
      "aria-valuenow",
      "5",
    );
    expect(screen.getAllByRole("tabpanel")[0]).toHaveTextContent(
      "hermes.memory.remaining",
    );
  });

  it("opens the Hermes Web UI config page through the caller's handler", () => {
    const onOpen = vi.fn();
    renderPage(onOpen);

    fireEvent.click(
      screen.getByRole("button", { name: /hermes.memory.openConfig/ }),
    );
    expect(onOpen).toHaveBeenCalledWith("/config");
    expect(mocks.openWebUI).not.toHaveBeenCalled();
  });
});
