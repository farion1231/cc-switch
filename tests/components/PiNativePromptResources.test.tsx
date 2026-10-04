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
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  PiPromptTemplates,
  PiSystemPromptFiles,
  type PiPromptTemplatesHandle,
} from "@/components/prompts/PiNativePromptResources";
import { promptsApi, type PiPromptFileKind } from "@/lib/api/prompts";

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: toastMocks.success,
    error: toastMocks.error,
    dismiss: vi.fn(),
  },
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

function lastUndo(): (() => void) | undefined {
  return toastMocks.success.mock.calls.at(-1)?.[1]?.action?.onClick;
}

describe("Pi native prompt resources", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    toastMocks.success.mockReset();
    toastMocks.error.mockReset();
    vi.spyOn(promptsApi, "getPiPromptFile").mockImplementation(
      async (kind: PiPromptFileKind) => ({
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
      async (_kind, _revision, content) => ({
        exists: true,
        revision: "saved-revision",
        content,
      }),
    );
  });

  it("shows file configuration state and edits the recommended append file", async () => {
    renderWithQueryClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.configuredSize");
    expect(screen.getByText("pi.prompts.notConfigured")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "common.edit APPEND_SYSTEM.md" }),
    );
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

    await screen.findByText("pi.prompts.configuredSize");
    fireEvent.click(
      screen.getByRole("button", { name: "common.edit APPEND_SYSTEM.md" }),
    );
    const editor = screen.getByPlaceholderText(
      "pi.prompts.instructionPlaceholder",
    );
    fireEvent.change(editor, { target: { value: "local draft" } });

    act(() => {
      queryClient.setQueryData(["pi", "promptFile", "system_append"], {
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

  it("explains a blank system file only after a save attempt", async () => {
    renderWithQueryClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.notConfigured");
    fireEvent.click(
      screen.getByRole("button", { name: "pi.prompts.create SYSTEM.md" }),
    );
    expect(
      screen.queryByText("pi.prompts.blankInstruction"),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "pi.prompts.saveAndConfigureEllipsis",
      }),
    );

    expect(screen.getByText("pi.prompts.blankInstruction")).toBeInTheDocument();
    expect(promptsApi.replacePiPromptFile).not.toHaveBeenCalled();
  });

  it("requires confirmation before creating SYSTEM.md", async () => {
    renderWithQueryClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.notConfigured");
    fireEvent.click(
      screen.getByRole("button", { name: "pi.prompts.create SYSTEM.md" }),
    );
    fireEvent.change(
      screen.getByPlaceholderText("pi.prompts.instructionPlaceholder"),
      { target: { value: "replace the system prompt" } },
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "pi.prompts.saveAndConfigureEllipsis",
      }),
    );

    expect(promptsApi.replacePiPromptFile).not.toHaveBeenCalled();
    const dialog = screen
      .getByText("pi.prompts.activateOverrideTitle")
      .closest('[role="dialog"]');
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

  it("deletes SYSTEM.md without a confirmation and writes it back on undo", async () => {
    vi.spyOn(promptsApi, "getPiPromptFile").mockImplementation(
      async (kind: PiPromptFileKind) => ({
        exists: kind === "system_override",
        revision: kind === "system_override" ? "system-revision" : "missing",
        content: kind === "system_override" ? "custom system prompt" : "",
      }),
    );
    const remove = vi
      .spyOn(promptsApi, "deletePiPromptFile")
      .mockResolvedValue(true);
    renderWithQueryClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.configuredSize");
    fireEvent.click(
      screen.getByRole("button", { name: "common.edit SYSTEM.md" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "pi.prompts.deleteFile" }),
    );

    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith("system_override", "system-revision"),
    );
    await waitFor(() =>
      expect(toastMocks.success).toHaveBeenCalledWith(
        "pi.prompts.overrideDeleted",
        expect.anything(),
      ),
    );
    act(() => lastUndo()?.());
    await waitFor(() =>
      expect(promptsApi.replacePiPromptFile).toHaveBeenCalledWith(
        "system_override",
        "missing",
        "custom system prompt",
      ),
    );
  });

  it("offers no undo when the deleted file was blank", async () => {
    vi.spyOn(promptsApi, "getPiPromptFile").mockImplementation(
      async (kind: PiPromptFileKind) => ({
        exists: kind === "system_append",
        revision: kind === "system_append" ? "blank-revision" : "missing",
        content: kind === "system_append" ? "  \n" : "",
      }),
    );
    vi.spyOn(promptsApi, "deletePiPromptFile").mockResolvedValue(true);
    renderWithQueryClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.configuredEmpty");
    fireEvent.click(
      screen.getByRole("button", { name: "common.edit APPEND_SYSTEM.md" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "pi.prompts.deleteFile" }),
    );

    await waitFor(() =>
      expect(toastMocks.success).toHaveBeenCalledWith(
        "pi.prompts.appendDeleted",
        expect.objectContaining({
          description: expect.stringContaining("pi.prompts.deletedBlank"),
        }),
      ),
    );
    expect(lastUndo()).toBeUndefined();
  });

  it("creates an empty native template from the contextual entry", async () => {
    renderWithQueryClient(<TemplateHarness />);

    await screen.findByText("/empty");
    fireEvent.click(screen.getByRole("button", { name: "open-create" }));
    fireEvent.change(screen.getByPlaceholderText("pi.prompts.templateSlug"), {
      target: { value: "new-empty" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "pi.prompts.createTemplate" }),
    );

    await waitFor(() =>
      expect(promptsApi.upsertPiPromptTemplate).toHaveBeenCalledWith(
        "new-empty",
        "missing",
        "",
        undefined,
      ),
    );
  });

  it("renames an existing slash-command template while saving it", async () => {
    vi.spyOn(promptsApi, "upsertPiPromptTemplate").mockResolvedValue({
      slug: "renamed",
      content: "",
      revision: "renamed-revision",
    });
    renderWithQueryClient(<TemplateHarness />);

    const edit = await screen.findByRole("button", {
      name: "prompts.editAria",
    });
    fireEvent.click(edit);
    const slug = screen.getByPlaceholderText("pi.prompts.templateSlug");
    fireEvent.change(slug, { target: { value: "renamed" } });
    expect(screen.getByText("pi.prompts.templateRename")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() =>
      expect(promptsApi.upsertPiPromptTemplate).toHaveBeenCalledWith(
        "renamed",
        "empty-revision",
        "",
        "empty",
      ),
    );
  });

  it("edits Pi description as notes without duplicating it in the body", async () => {
    vi.spyOn(promptsApi, "listPiPromptTemplates").mockResolvedValue([
      {
        slug: "review",
        content:
          '---\ndescription: "Existing note"\nargument-hint: "<target>"\n---\nReview $1',
        revision: "review-revision",
      },
    ]);
    vi.spyOn(promptsApi, "upsertPiPromptTemplate").mockResolvedValue({
      slug: "review",
      content:
        '---\ndescription: "Updated note"\nargument-hint: "<target>"\n---\nReview $1',
      revision: "updated-revision",
    });
    renderWithQueryClient(<TemplateHarness />);

    expect(await screen.findByText("Existing note")).toBeInTheDocument();
    expect(screen.getByText("<target>")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "prompts.editAria" }));

    const notes = screen.getByPlaceholderText(
      "pi.prompts.templateDescriptionPlaceholder",
    );
    const body = screen.getByPlaceholderText(
      "pi.prompts.templateContentPlaceholder",
    );
    expect(notes).toHaveValue("Existing note");
    expect(body).toHaveValue('---\nargument-hint: "<target>"\n---\nReview $1');

    fireEvent.change(notes, { target: { value: "Updated note" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() =>
      expect(promptsApi.upsertPiPromptTemplate).toHaveBeenCalledWith(
        "review",
        "review-revision",
        '---\ndescription: "Updated note"\nargument-hint: "<target>"\n---\nReview $1',
        "review",
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
      fireEvent.click(create);
      expect(
        screen.getByText("pi.prompts.templateSlugInvalid"),
      ).toBeInTheDocument();
    }
    fireEvent.change(slug, { target: { value: "empty" } });
    expect(
      screen.getByText("pi.prompts.templateSlugExists"),
    ).toBeInTheDocument();
    expect(promptsApi.upsertPiPromptTemplate).not.toHaveBeenCalled();

    fireEvent.change(slug, { target: { value: "release.v2" } });
    expect(screen.getByText("pi.prompts.templateSaveTo")).toBeInTheDocument();
    fireEvent.click(create);
    await waitFor(() =>
      expect(promptsApi.upsertPiPromptTemplate).toHaveBeenCalledWith(
        "release.v2",
        "missing",
        "",
        undefined,
      ),
    );
  });

  it("deletes a template from its row menu and writes it back on undo", async () => {
    const remove = vi
      .spyOn(promptsApi, "deletePiPromptTemplate")
      .mockResolvedValue(true);
    renderWithQueryClient(<TemplateHarness />);
    const user = userEvent.setup();

    await screen.findByText("/empty");
    await user.click(
      screen.getByRole("button", { name: "prompts.rowMoreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: "common.delete" }),
    );

    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith("empty", "empty-revision"),
    );
    await waitFor(() =>
      expect(toastMocks.success).toHaveBeenCalledWith(
        "pi.prompts.templateDeletedUndo",
        expect.anything(),
      ),
    );
    act(() => lastUndo()?.());
    await waitFor(() =>
      expect(promptsApi.upsertPiPromptTemplate).toHaveBeenCalledWith(
        "empty",
        "missing",
        "",
      ),
    );
  });
});
