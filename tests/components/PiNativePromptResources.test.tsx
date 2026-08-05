import { useRef, type ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  PiPromptTemplates,
  PiSystemPromptFiles,
  type PiPromptTemplatesHandle,
} from "@/components/prompts/PiNativePromptResources";
import { promptsApi, type PiPromptFileKind } from "@/lib/api/prompts";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@/components/MarkdownEditor", () => ({
  default: ({
    value,
    onChange,
    placeholder,
  }: {
    value: string;
    onChange?: (value: string) => void;
    placeholder?: string;
  }) => (
    <textarea
      value={value}
      placeholder={placeholder}
      onChange={(event) => onChange?.(event.target.value)}
    />
  ),
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    title,
    children,
    footer,
  }: {
    isOpen: boolean;
    title: string;
    children: ReactNode;
    footer?: ReactNode;
  }) =>
    isOpen ? (
      <section aria-label={title}>
        {children}
        <footer>{footer}</footer>
      </section>
    ) : null,
}));

const createClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

const renderWithQueryClient = (
  ui: ReactNode,
  queryClient = createClient(),
) => ({
  queryClient,
  ...render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  ),
});

function TemplateHarness() {
  const ref = useRef<PiPromptTemplatesHandle>(null);
  return (
    <>
      <button type="button" onClick={() => ref.current?.openCreate()}>
        open-create
      </button>
      <PiPromptTemplates ref={ref} />
    </>
  );
}

describe("Pi native prompt resources", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(promptsApi, "getPiPromptFile").mockImplementation(
      async (kind: PiPromptFileKind) => ({
        kind,
        path:
          kind === "system_override"
            ? "/agent/SYSTEM.md"
            : "/agent/APPEND_SYSTEM.md",
        exists: kind === "system_append",
        revision: kind === "system_append" ? "append-revision" : "missing",
        content: kind === "system_append" ? "append" : "",
      }),
    );
    vi.spyOn(promptsApi, "listPiPromptTemplates").mockResolvedValue([
      {
        slug: "empty",
        content: "",
        revision: "empty-revision",
      },
    ]);
    vi.spyOn(promptsApi, "upsertPiPromptTemplate").mockResolvedValue({
      slug: "new-empty",
      content: "",
      revision: "created-revision",
    });
    vi.spyOn(promptsApi, "replacePiPromptFile").mockImplementation(
      async (kind, _revision, content) => ({
        kind,
        path:
          kind === "system_override"
            ? "/agent/SYSTEM.md"
            : "/agent/APPEND_SYSTEM.md",
        exists: true,
        revision: "saved-revision",
        content,
      }),
    );
  });

  it("shows file configuration state and edits the recommended append file", async () => {
    renderWithQueryClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.configured");
    expect(screen.getByText("pi.prompts.notConfigured")).toBeInTheDocument();
    expect(screen.queryByText("pi.prompts.active")).not.toBeInTheDocument();

    fireEvent.click(screen.getByText("APPEND_SYSTEM.md").closest("button")!);
    const editor = screen.getByPlaceholderText(
      "pi.prompts.instructionPlaceholder",
    );
    fireEvent.change(editor, { target: { value: "new append" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() =>
      expect(promptsApi.replacePiPromptFile).toHaveBeenCalledWith(
        "system_append",
        "append-revision",
        "new append",
      ),
    );
  });

  it("keeps the open draft and its base revision when the query refreshes", async () => {
    const { queryClient } = renderWithQueryClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.configured");
    fireEvent.click(screen.getByText("APPEND_SYSTEM.md").closest("button")!);
    const editor = screen.getByPlaceholderText(
      "pi.prompts.instructionPlaceholder",
    );
    fireEvent.change(editor, { target: { value: "local draft" } });

    act(() => {
      queryClient.setQueryData(["pi", "promptFile", "system_append"], {
        kind: "system_append",
        path: "/agent/APPEND_SYSTEM.md",
        exists: true,
        revision: "external-revision",
        content: "external edit",
      });
    });

    expect(editor).toHaveValue("local draft");
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(promptsApi.replacePiPromptFile).toHaveBeenCalledWith(
        "system_append",
        "append-revision",
        "local draft",
      ),
    );
  });

  it("creates an empty native template from the contextual entry", async () => {
    renderWithQueryClient(<TemplateHarness />);

    await screen.findByText("/empty");
    fireEvent.click(screen.getByRole("button", { name: "open-create" }));
    fireEvent.change(screen.getByPlaceholderText("pi.prompts.templateSlug"), {
      target: { value: "new-empty" },
    });

    const create = screen.getByRole("button", {
      name: "pi.prompts.createTemplate",
    });
    expect(create).toBeEnabled();
    fireEvent.click(create);

    await waitFor(() =>
      expect(promptsApi.upsertPiPromptTemplate).toHaveBeenCalledWith(
        "new-empty",
        "missing",
        "",
      ),
    );
  });

  it("rejects prompt-template names that Pi cannot invoke portably", async () => {
    renderWithQueryClient(<TemplateHarness />);

    await screen.findByText("/empty");
    fireEvent.click(screen.getByRole("button", { name: "open-create" }));
    const slug = screen.getByPlaceholderText("pi.prompts.templateSlug");
    const create = screen.getByRole("button", {
      name: "pi.prompts.createTemplate",
    });

    for (const invalid of ["release notes", "bad:name", "CON"]) {
      fireEvent.change(slug, { target: { value: invalid } });
      expect(create).toBeDisabled();
      expect(
        screen.getByText("pi.prompts.templateSlugInvalid"),
      ).toBeInTheDocument();
    }

    fireEvent.change(slug, { target: { value: "release.v2" } });
    expect(create).toBeEnabled();
  });

  it("requires confirmation before creating SYSTEM.md", async () => {
    renderWithQueryClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.notConfigured");
    fireEvent.click(screen.getByText("SYSTEM.md").closest("button")!);
    fireEvent.change(
      screen.getByPlaceholderText("pi.prompts.instructionPlaceholder"),
      {
        target: { value: "replace the system prompt" },
      },
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "pi.prompts.saveAndConfigure",
      }),
    );

    expect(promptsApi.replacePiPromptFile).not.toHaveBeenCalled();
    const dialogTitle = screen.getByText("pi.prompts.activateOverrideTitle");
    const dialog = dialogTitle.closest('[role="dialog"]');
    expect(dialog).not.toBeNull();
    fireEvent.click(
      within(dialog as HTMLElement).getByRole("button", {
        name: "pi.prompts.saveAndConfigure",
      }),
    );

    await waitFor(() =>
      expect(promptsApi.replacePiPromptFile).toHaveBeenCalledWith(
        "system_override",
        "missing",
        "replace the system prompt",
      ),
    );
  });
});
