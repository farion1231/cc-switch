import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
} from "@testing-library/react";
import type { McpServer } from "@/types";
import McpFormModal from "@/components/mcp/McpFormModal";

const toastErrorMock = vi.hoisted(() => vi.fn());
const toastSuccessMock = vi.hoisted(() => vi.fn());
const upsertMock = vi.hoisted(() => {
  const fn = vi.fn();
  fn.mockResolvedValue(undefined);
  return fn;
});

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params ? `${key}:${JSON.stringify(params)}` : key,
  }),
  // 提供 initReactI18next 以兼容 i18n 初始化路径
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

vi.mock("@/config/mcpPresets", () => ({
  mcpPresets: [
    {
      id: "preset-stdio",
      server: { type: "stdio", command: "preset-cmd" },
    },
  ],
  getMcpPresetWithDescription: (preset: any) => ({
    ...preset,
    description: "Preset description",
    tags: ["preset"],
  }),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, onClick, type = "button", ...rest }: any) => (
    <button type={type} onClick={onClick} {...rest}>
      {children}
    </button>
  ),
}));

vi.mock("@/components/ui/input", () => ({
  Input: ({ value, onChange, ...rest }: any) => (
    <input
      value={value}
      onChange={(event) =>
        onChange?.({ target: { value: event.target.value } })
      }
      {...rest}
    />
  ),
}));

vi.mock("@/components/ui/textarea", () => ({
  Textarea: ({ value, onChange, ...rest }: any) => (
    <textarea
      value={value}
      onChange={(event) =>
        onChange?.({ target: { value: event.target.value } })
      }
      {...rest}
    />
  ),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value, onChange, placeholder, ...rest }: any) => (
    <textarea
      value={value}
      placeholder={placeholder}
      onChange={(event) => onChange?.(event.target.value)}
      {...rest}
    />
  ),
}));

vi.mock("@/components/ui/checkbox", () => ({
  Checkbox: ({ id, checked, onCheckedChange, ...rest }: any) => (
    <input
      type="checkbox"
      id={id}
      checked={checked ?? false}
      onChange={(e) => onCheckedChange?.(e.target.checked)}
      {...rest}
    />
  ),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ children }: any) => <div>{children}</div>,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("@/components/mcp/McpWizardModal", () => ({
  default: ({ isOpen, onApply }: any) =>
    isOpen ? (
      <button
        type="button"
        data-testid="wizard-apply"
        onClick={() =>
          onApply(
            "wizard-id",
            JSON.stringify({ type: "stdio", command: "wizard-cmd" }),
          )
        }
      >
        wizard-apply
      </button>
    ) : null,
}));

vi.mock("@/hooks/useMcp", async () => {
  const actual =
    await vi.importActual<typeof import("@/hooks/useMcp")>("@/hooks/useMcp");
  return {
    ...actual,
    useUpsertMcpServer: () => ({
      mutateAsync: (...args: unknown[]) => upsertMock(...args),
    }),
  };
});

describe("McpFormModal", () => {
  beforeEach(() => {
    toastErrorMock.mockClear();
    toastSuccessMock.mockClear();
    upsertMock.mockClear();
  });

  const renderForm = (
    props?: Partial<React.ComponentProps<typeof McpFormModal>>,
  ) => {
    const {
      onSave: overrideOnSave,
      onClose: overrideOnClose,
      ...rest
    } = props ?? {};
    const onSave = overrideOnSave ?? vi.fn().mockResolvedValue(undefined);
    const onClose = overrideOnClose ?? vi.fn();
    render(
      <McpFormModal
        onSave={onSave}
        onClose={onClose}
        existingIds={[]}
        defaultFormat="json"
        {...rest}
      />,
    );
    return { onSave, onClose };
  };

  it("应用预设后填充 ID 与配置内容", async () => {
    renderForm();
    await waitFor(() =>
      expect(
        screen.getByPlaceholderText("mcp.form.titlePlaceholder"),
      ).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByText("preset-stdio"));

    const idInput = screen.getByPlaceholderText(
      "mcp.form.titlePlaceholder",
    ) as HTMLInputElement;
    expect(idInput.value).toBe("preset-stdio");

    const configTextarea = screen.getByPlaceholderText(
      "mcp.form.jsonPlaceholder",
    ) as HTMLTextAreaElement;
    expect(configTextarea.value).toBe(
      '{\n  "type": "stdio",\n  "command": "preset-cmd"\n}',
    );
  });

  it("提交时清洗字段并调用 upsert 与 onSave", async () => {
    const { onSave } = renderForm();

    fireEvent.change(screen.getByPlaceholderText("mcp.form.titlePlaceholder"), {
      target: { value: " my-server " },
    });
    fireEvent.change(screen.getByPlaceholderText("mcp.form.namePlaceholder"), {
      target: { value: "   Friendly " },
    });

    fireEvent.click(screen.getByText("mcp.form.additionalInfo"));

    fireEvent.change(
      screen.getByPlaceholderText("mcp.form.descriptionPlaceholder"),
      {
        target: { value: " Description " },
      },
    );
    fireEvent.change(screen.getByPlaceholderText("mcp.form.tagsPlaceholder"), {
      target: { value: " tag1 , tag2 " },
    });
    fireEvent.change(
      screen.getByPlaceholderText("mcp.form.homepagePlaceholder"),
      {
        target: { value: " https://example.com " },
      },
    );
    fireEvent.change(screen.getByPlaceholderText("mcp.form.docsPlaceholder"), {
      target: { value: " https://docs.example.com " },
    });

    fireEvent.change(screen.getByPlaceholderText("mcp.form.jsonPlaceholder"), {
      target: { value: '{"type":"stdio","command":"run"}' },
    });

    fireEvent.click(screen.getByText("common.add"));

    await waitFor(() => expect(upsertMock).toHaveBeenCalledTimes(1));
    const [entry] = upsertMock.mock.calls.at(-1) ?? [];
    expect(entry).toMatchObject({
      id: "my-server",
      name: "Friendly",
      description: "Description",
      homepage: "https://example.com",
      docs: "https://docs.example.com",
      tags: ["tag1", "tag2"],
      server: {
        type: "stdio",
        command: "run",
      },
      apps: {
        claude: true,
        codex: true,
        gemini: true,
      },
    });
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith();
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("缺少配置命令时阻止提交并提示错误", async () => {
    renderForm();

    fireEvent.change(screen.getByPlaceholderText("mcp.form.titlePlaceholder"), {
      target: { value: "no-command" },
    });
    fireEvent.change(screen.getByPlaceholderText("mcp.form.jsonPlaceholder"), {
      target: { value: '{"type":"stdio"}' },
    });

    fireEvent.click(screen.getByText("common.add"));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(upsertMock).not.toHaveBeenCalled();
    const [message] = toastErrorMock.mock.calls.at(-1) ?? [];
    expect(message).toBe("mcp.error.commandRequired");
  });

  it("支持向导生成配置并自动填充 ID", async () => {
    renderForm();
    fireEvent.click(screen.getByText("mcp.form.useWizard"));

    const applyButton = await screen.findByTestId("wizard-apply");
    await act(async () => {
      fireEvent.click(applyButton);
    });

    const idInput = screen.getByPlaceholderText(
      "mcp.form.titlePlaceholder",
    ) as HTMLInputElement;
    expect(idInput.value).toBe("wizard-id");

    const configTextarea = screen.getByPlaceholderText(
      "mcp.form.jsonPlaceholder",
    ) as HTMLTextAreaElement;
    expect(configTextarea.value).toBe(
      '{"type":"stdio","command":"wizard-cmd"}',
    );
  });

  it("TOML 模式下自动提取 ID 并成功保存", async () => {
    const { onSave } = renderForm({ defaultFormat: "toml" });

    const configTextarea = screen.getByPlaceholderText(
      "mcp.form.tomlPlaceholder",
    ) as HTMLTextAreaElement;

    const toml = `[mcp.servers.demo]
type = "stdio"
command = "run"
`;
    fireEvent.change(configTextarea, { target: { value: toml } });

    const idInput = screen.getByPlaceholderText(
      "mcp.form.titlePlaceholder",
    ) as HTMLInputElement;

    await waitFor(() => expect(idInput.value).toBe("demo"));

    fireEvent.click(screen.getByText("common.add"));

    await waitFor(() => expect(upsertMock).toHaveBeenCalledTimes(1));
    const [entry] = upsertMock.mock.calls.at(-1) ?? [];
    expect(entry.id).toBe("demo");
    expect(entry.server).toEqual({ type: "stdio", command: "run" });
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith();
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("TOML 模式下缺少命令时展示错误提示并阻止提交", async () => {
    renderForm({ defaultFormat: "toml" });

    // 填写 ID 字段
    fireEvent.change(screen.getByPlaceholderText("mcp.form.titlePlaceholder"), {
      target: { value: "test-toml" },
    });

    const configTextarea = screen.getByPlaceholderText(
      "mcp.form.tomlPlaceholder",
    ) as HTMLTextAreaElement;

    const invalidToml = `[mcp.servers.demo]
type = "stdio"
`;
    fireEvent.change(configTextarea, { target: { value: invalidToml } });

    fireEvent.click(screen.getByText("common.add"));

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("mcp.error.tomlInvalid", {
        duration: 3000,
      }),
    );
    expect(upsertMock).not.toHaveBeenCalled();
  });

  it("编辑模式下保持 ID 并更新配置", async () => {
    const initialData: McpServer = {
      id: "existing",
      name: "Existing",
      enabled: true,
      description: "Old desc",
      server: { type: "stdio", command: "old" },
      apps: { claude: true, codex: false, gemini: false },
    } as McpServer;

    const { onSave } = renderForm({
      editingId: "existing",
      initialData,
    });

    const idInput = screen.getByPlaceholderText(
      "mcp.form.titlePlaceholder",
    ) as HTMLInputElement;
    expect(idInput.value).toBe("existing");
    expect(idInput).toHaveAttribute("disabled");

    const configTextarea = screen.getByPlaceholderText(
      "mcp.form.jsonPlaceholder",
    ) as HTMLTextAreaElement;
    expect(configTextarea.value).toContain('"command": "old"');

    fireEvent.change(configTextarea, {
      target: { value: '{"type":"stdio","command":"updated"}' },
    });

    fireEvent.click(screen.getByText("common.save"));

    await waitFor(() => expect(upsertMock).toHaveBeenCalledTimes(1));
    const [entry] = upsertMock.mock.calls.at(-1) ?? [];
    expect(entry.id).toBe("existing");
    expect(entry.server.command).toBe("updated");
    expect(entry.enabled).toBe(true);
    expect(entry.apps).toEqual({
      claude: true,
      codex: false,
      gemini: false,
    });
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith();
  });

  it("允许未选择任何应用保存配置，并保持 apps 全 false", async () => {
    const { onSave } = renderForm();

    fireEvent.change(screen.getByPlaceholderText("mcp.form.titlePlaceholder"), {
      target: { value: "no-apps" },
    });
    fireEvent.change(screen.getByPlaceholderText("mcp.form.jsonPlaceholder"), {
      target: { value: '{"type":"stdio","command":"run"}' },
    });

    const claudeCheckbox = screen.getByLabelText(
      "mcp.unifiedPanel.apps.claude",
    ) as HTMLInputElement;
    expect(claudeCheckbox.checked).toBe(true);
    fireEvent.click(claudeCheckbox);

    const codexCheckbox = screen.getByLabelText(
      "mcp.unifiedPanel.apps.codex",
    ) as HTMLInputElement;
    expect(codexCheckbox.checked).toBe(true);
    fireEvent.click(codexCheckbox);

    const geminiCheckbox = screen.getByLabelText(
      "mcp.unifiedPanel.apps.gemini",
    ) as HTMLInputElement;
    expect(geminiCheckbox.checked).toBe(true);
    fireEvent.click(geminiCheckbox);

    fireEvent.click(screen.getByText("common.add"));

    await waitFor(() => expect(upsertMock).toHaveBeenCalledTimes(1));
    const [entry] = upsertMock.mock.calls.at(-1) ?? [];
    expect(entry.id).toBe("no-apps");
    expect(entry.apps).toEqual({
      claude: false,
      codex: false,
      gemini: false,
    });
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("保存失败时展示翻译后的错误并恢复按钮", async () => {
    const failingSave = vi.fn().mockRejectedValue(new Error("保存失败"));
    renderForm({ onSave: failingSave });

    fireEvent.change(screen.getByPlaceholderText("mcp.form.titlePlaceholder"), {
      target: { value: "will-fail" },
    });
    fireEvent.change(screen.getByPlaceholderText("mcp.form.jsonPlaceholder"), {
      target: { value: '{"type":"stdio","command":"ok"}' },
    });

    fireEvent.click(screen.getByText("common.add"));

    await waitFor(() => expect(failingSave).toHaveBeenCalled());
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    const [message] = toastErrorMock.mock.calls.at(-1) ?? [];
    expect(message).toBe("保存失败");

    const addButton = screen.getByText("common.add") as HTMLButtonElement;
    expect(addButton.disabled).toBe(false);
  });
});
