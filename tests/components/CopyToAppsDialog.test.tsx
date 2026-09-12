import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CopyToAppsDialog } from "@/components/providers/CopyToAppsDialog";
import type { AppId } from "@/lib/api/types";
import type { Provider } from "@/types";

const { dialogState, tMock } = vi.hoisted(() => ({
  dialogState: {
    openChange: null as null | ((open: boolean) => void),
  },
  tMock: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

// 与 AddProviderDialog.test.tsx 相同的透传 mock；额外保留 open 语义与
// onOpenChange 句柄，用于验证"复制进行中禁止关闭"。
vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({
    open,
    onOpenChange,
    children,
  }: {
    open: boolean;
    onOpenChange?: (open: boolean) => void;
    children: React.ReactNode;
  }) => {
    dialogState.openChange = onOpenChange ?? null;
    return open ? <div data-testid="dialog-root">{children}</div> : null;
  },
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h2>{children}</h2>
  ),
  DialogDescription: ({ children }: { children: React.ReactNode }) => (
    <p>{children}</p>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

const TRANSLATIONS: Record<string, string> = {
  "provider.copyToApps.title": "复制到其他应用",
  "provider.copyToApps.description": '将"{{name}}"的配置复制到选定的应用中',
  "provider.copyToApps.selectAll": "全选",
  "provider.copyToApps.selectedCount": "已选 {{count}} / {{total}}",
  "provider.copyToApps.copy": "复制到 {{count}} 个应用",
  "provider.copyToApps.copying": "正在复制…",
  "provider.copyToApps.modelHint": "复制后请核对模型",
  "common.cancel": "取消",
};

tMock.mockImplementation((key: string, params?: Record<string, unknown>) => {
  const template =
    TRANSLATIONS[key] ??
    (typeof params?.defaultValue === "string" ? params.defaultValue : key);
  return template.replace(/\{\{(\w+)\}\}/g, (_match, name: string) =>
    params && name in params ? String(params[name]) : `{{${name}}}`,
  );
});

const makeProvider = (overrides: Partial<Provider> = {}): Provider => ({
  id: "relay-src",
  name: "Relay",
  settingsConfig: {},
  category: "custom",
  createdAt: Date.now(),
  ...overrides,
});

const renderDialog = (
  props: Partial<React.ComponentProps<typeof CopyToAppsDialog>> = {},
) => {
  const onClose = vi.fn();
  const onCopy = vi.fn(async (_provider: Provider, _targets: AppId[]) => {});
  const utils = render(
    <CopyToAppsDialog
      isOpen
      onClose={onClose}
      provider={makeProvider()}
      sourceApp="claude"
      onCopy={onCopy}
      {...props}
    />,
  );
  return { onClose, onCopy, ...utils };
};

const checkbox = (name: string) =>
  screen.getByRole("checkbox", { name }) as HTMLInputElement;

describe("CopyToAppsDialog", () => {
  it("lists every other app for a Claude source, excluding the source itself", () => {
    renderDialog();

    // 源应用 Claude 自身不出现；其余 8 个应用按 APP_IDS 顺序全部出现。
    expect(
      screen.queryByRole("checkbox", { name: "Claude" }),
    ).not.toBeInTheDocument();
    for (const name of [
      "Claude Desktop",
      "Codex",
      "Gemini",
      "Grok Build",
      "OpenCode",
      "OpenClaw",
      "Hermes",
      "Pi",
    ]) {
      expect(checkbox(name)).toBeInTheDocument();
    }
    expect(screen.getByText("已选 0 / 8")).toBeInTheDocument();
  });

  it("offers Claude Desktop only when copying from Claude", () => {
    renderDialog({ sourceApp: "codex" });

    // Claude Desktop 只接受 Claude 形状的配置，非 Claude 源不可见；
    // 目标为其余 7 个应用（源 Codex 与 Claude Desktop 均排除）。
    expect(
      screen.queryByRole("checkbox", { name: "Codex" }),
    ).not.toBeInTheDocument();
    expect(checkbox("Claude")).toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", { name: "Claude Desktop" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("已选 0 / 7")).toBeInTheDocument();
  });

  it("renders nothing while closed", () => {
    renderDialog({ isOpen: false });

    expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument();
  });

  it("describes the copied provider by name", () => {
    renderDialog({ provider: makeProvider({ name: "My Relay" }) });

    expect(
      screen.getByText('将"My Relay"的配置复制到选定的应用中'),
    ).toBeInTheDocument();
  });

  it("keeps copy disabled until a target is selected", async () => {
    const user = userEvent.setup();
    renderDialog();

    const submit = screen.getByRole("button", { name: "复制到 0 个应用" });
    expect(submit).toBeDisabled();

    await user.click(checkbox("Codex"));
    expect(
      screen.getByRole("button", { name: "复制到 1 个应用" }),
    ).toBeEnabled();
  });

  it("updates the counter and copy label for each picked app", async () => {
    const user = userEvent.setup();
    renderDialog();

    await user.click(checkbox("Gemini"));
    await user.click(checkbox("Pi"));

    expect(screen.getByText("已选 2 / 8")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "复制到 2 个应用" }),
    ).toBeInTheDocument();

    await user.click(checkbox("Pi"));
    expect(screen.getByText("已选 1 / 8")).toBeInTheDocument();
  });

  it("selects every target via select-all and clears on the second click", async () => {
    const user = userEvent.setup();
    renderDialog();

    await user.click(checkbox("全选"));
    expect(screen.getByText("已选 8 / 8")).toBeInTheDocument();
    for (const name of ["Claude Desktop", "Codex", "Gemini", "Pi"]) {
      expect(checkbox(name).checked).toBe(true);
    }

    await user.click(checkbox("全选"));
    expect(screen.getByText("已选 0 / 8")).toBeInTheDocument();
    expect(checkbox("Codex").checked).toBe(false);
  });

  it("submits only the selected targets in list order and closes", async () => {
    const user = userEvent.setup();
    const { onCopy, onClose } = renderDialog();

    await user.click(checkbox("Gemini"));
    await user.click(checkbox("Codex"));
    await user.click(screen.getByRole("button", { name: "复制到 2 个应用" }));

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(onCopy).toHaveBeenCalledTimes(1);
    expect(onCopy.mock.calls[0][0].id).toBe("relay-src");
    expect(onCopy.mock.calls[0][1]).toEqual(["codex", "gemini"]);
  });

  it("keeps the dialog open and recovers when onCopy rejects", async () => {
    const user = userEvent.setup();
    const onCopy = vi.fn(async () => {
      throw new Error("boom");
    });
    const { onClose } = renderDialog({ onCopy });

    await user.click(checkbox("OpenClaw"));
    await user.click(screen.getByRole("button", { name: "复制到 1 个应用" }));

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "复制到 1 个应用" }),
      ).toBeEnabled(),
    );
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("dialog-root")).toBeInTheDocument();
  });

  it("disables the footer and blocks closing while a copy is in flight", async () => {
    const user = userEvent.setup();
    let resolveCopy: (() => void) | undefined;
    const onCopy = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveCopy = resolve;
        }),
    );
    const { onClose } = renderDialog({ onCopy });

    await user.click(checkbox("Codex"));
    await user.click(screen.getByRole("button", { name: "复制到 1 个应用" }));

    expect(screen.getByRole("button", { name: "正在复制…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();

    // 复制进行中：请求关闭应被忽略。
    dialogState.openChange?.(false);
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => {
      resolveCopy?.();
    });

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("resets the selection every time the dialog reopens", async () => {
    const user = userEvent.setup();
    const props = {
      onClose: vi.fn(),
      onCopy: vi.fn(async () => {}),
    };
    const { rerender } = render(
      <CopyToAppsDialog
        isOpen
        provider={makeProvider()}
        sourceApp="claude"
        {...props}
      />,
    );

    await user.click(checkbox("Hermes"));
    expect(screen.getByText("已选 1 / 8")).toBeInTheDocument();

    rerender(
      <CopyToAppsDialog
        isOpen={false}
        provider={makeProvider()}
        sourceApp="claude"
        {...props}
      />,
    );
    rerender(
      <CopyToAppsDialog
        isOpen
        provider={makeProvider()}
        sourceApp="claude"
        {...props}
      />,
    );

    expect(screen.getByText("已选 0 / 8")).toBeInTheDocument();
    expect(checkbox("Hermes").checked).toBe(false);
  });
});
