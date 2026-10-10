import { createRef } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderWithQueryClient } from "../utils/testQueryClient";
import {
  CopilotByokSettings,
  type CopilotByokSettingsHandle,
} from "@/components/settings/CopilotByokSettings";
import { copilotByokApi, type CopilotByokState } from "@/lib/api/copilotByok";
import { toast } from "@/lib/toast";
import { hasUnsavedChanges } from "@/lib/unsavedChanges";
import { pngDataUri } from "../fixtures/customIcon";

vi.mock("@/lib/api/copilotByok", () => ({
  copilotByokApi: {
    getState: vi.fn(),
    setTargets: vi.fn(),
    addCustomTarget: vi.fn(),
    removeCustomTarget: vi.fn(),
    upsertGroup: vi.fn(),
    deleteGroup: vi.fn(),
    sync: vi.fn(),
    restoreBackup: vi.fn(),
  },
}));
vi.mock("@/lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const state: CopilotByokState = {
  groups: [
    {
      id: "provider",
      name: "Provider",
      url: "https://example.com",
      apiKey: "secret",
      apiType: "chat-completions",
      enabled: true,
      requestHeaders: {},
      models: [
        {
          id: "model",
          modelId: "custom-model",
          name: "Custom model",
          enabled: true,
          extra: {},
        },
      ],
      extra: {},
    },
  ],
  targets: [
    {
      id: "profile",
      source: "custom",
      edition: null,
      editionName: null,
      profileId: null,
      profileName: "Profile",
      isDefault: false,
      languageModelsPath: "/profile/chatLanguageModels.json",
      configExists: true,
      backupExists: true,
      selected: true,
      managedGroupCount: 1,
      readError: null,
    },
  ],
  selectedTargetIds: ["profile"],
  managedModelCount: 1,
};

describe("VS Code Copilot BYOK core", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(copilotByokApi.getState).mockResolvedValue(
      structuredClone(state),
    );
    vi.mocked(copilotByokApi.upsertGroup).mockResolvedValue(state);
    vi.mocked(copilotByokApi.deleteGroup).mockResolvedValue(state);
    vi.mocked(copilotByokApi.setTargets).mockResolvedValue(state);
  });

  it("shows a usable catalog without CLI, resources or usage controls", async () => {
    renderWithQueryClient(<CopilotByokSettings />);
    expect(await screen.findByText("Provider")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "copilotByok.sync" }),
    ).toBeEnabled();
    expect(
      screen.queryByText(/Copilot CLI|MCP|Skills/),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("secret")).not.toBeInTheDocument();
  });

  it("shows saved provider icons with the shared compact item layout", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      groups: [
        { ...state.groups[0], icon: pngDataUri(96, 96), iconColor: "#123456" },
      ],
    });
    renderWithQueryClient(<CopilotByokSettings />);
    const item = await screen.findByRole("group", { name: "Provider" });
    const image = item.querySelector("img") as HTMLImageElement;
    expect(image.src).toBe(pngDataUri(96, 96));
    expect(image.style.width).toBe("32px");
    expect(image.className).toContain("object-cover");
    expect(image.parentElement).toHaveClass("bg-white", "h-8", "w-8");
    expect(item.firstElementChild).toHaveClass("min-h-[58px]", "gap-3");
  });

  it("shows notes as plain text instead of turning them into endpoint links", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      groups: [
        {
          ...state.groups[0],
          notes: "  Internal relay  ",
          websiteUrl: "https://provider.test",
        },
      ],
    });
    const onOpenWebsite = vi.fn();
    renderWithQueryClient(
      <CopilotByokSettings onOpenWebsite={onOpenWebsite} />,
    );
    fireEvent.click(await screen.findByText("Internal relay"));
    expect(onOpenWebsite).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Internal relay" })).toBeNull();
    expect(screen.queryByText("https://provider.test")).toBeNull();
    expect(screen.queryByText("https://example.com")).toBeNull();
  });

  it("prefers the website over the endpoint when no notes are present", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      groups: [
        {
          ...state.groups[0],
          notes: "  ",
          websiteUrl: "https://provider.test",
        },
      ],
    });
    const onOpenWebsite = vi.fn();
    renderWithQueryClient(
      <CopilotByokSettings onOpenWebsite={onOpenWebsite} />,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "https://provider.test" }),
    );
    expect(onOpenWebsite).toHaveBeenCalledWith("https://provider.test");
    expect(screen.queryByText("https://example.com")).toBeNull();
  });

  it("does not make unsupported website schemes clickable", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      groups: [{ ...state.groups[0], websiteUrl: "javascript:alert(1)" }],
    });
    renderWithQueryClient(<CopilotByokSettings onOpenWebsite={vi.fn()} />);
    expect(await screen.findByText("javascript:alert(1)")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "javascript:alert(1)" }),
    ).toBeNull();
  });

  it("searches notes and website metadata using the shared search controls", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      groups: [
        {
          ...state.groups[0],
          notes: "Internal relay",
          websiteUrl: "https://provider.test",
        },
      ],
    });
    renderWithQueryClient(<CopilotByokSettings searchOpen />);
    await screen.findByText("Provider");
    const search = screen.getByRole("textbox", {
      name: "provider.searchAriaLabel",
    });
    expect(search).toHaveAttribute("placeholder", "provider.searchPlaceholder");
    expect(screen.getByText("provider.searchScopeHint")).toBeInTheDocument();
    for (const keyword of ["RELAY", "PROVIDER.TEST"]) {
      fireEvent.change(search, { target: { value: keyword } });
      expect(
        screen.getByRole("group", { name: "Provider" }),
      ).toBeInTheDocument();
    }
    fireEvent.change(search, { target: { value: "missing" } });
    expect(screen.getByText("provider.noSearchResults")).toBeInTheDocument();
  });

  it("preserves display metadata when toggling and editing a provider", async () => {
    const group = {
      ...state.groups[0],
      icon: pngDataUri(96, 96),
      iconColor: "#123456",
      notes: "Internal relay",
      websiteUrl: "https://provider.test",
    };
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      groups: [group],
    });
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    fireEvent.click(
      screen.getByRole("button", { name: "provider.removeFromConfig" }),
    );
    await waitFor(() =>
      expect(copilotByokApi.upsertGroup).toHaveBeenCalledWith({
        ...group,
        enabled: false,
      }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "providerCard.action.edit",
        }),
      ).not.toHaveAttribute("aria-disabled"),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerCard.action.edit" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(copilotByokApi.upsertGroup).toHaveBeenLastCalledWith(group),
    );
  });

  it("creates an endpoint with a model through the editor", async () => {
    const ref = createRef<CopilotByokSettingsHandle>();
    renderWithQueryClient(<CopilotByokSettings ref={ref} />);
    await screen.findByText("Provider");
    act(() => ref.current?.openAdd());
    fireEvent.change(screen.getByLabelText("copilotByok.name"), {
      target: { value: "New provider" },
    });
    fireEvent.change(screen.getByLabelText("copilotByok.url"), {
      target: { value: "https://example.com/v1" },
    });
    fireEvent.change(screen.getByLabelText("copilotByok.apiKey"), {
      target: { value: "new-secret" },
    });
    fireEvent.change(screen.getByLabelText("copilotByok.modelId"), {
      target: { value: "model-1" },
    });
    fireEvent.change(screen.getByLabelText("copilotByok.modelName"), {
      target: { value: "Model 1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(copilotByokApi.upsertGroup).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "New provider",
          apiKey: "new-secret",
          models: [
            expect.objectContaining({ modelId: "model-1", name: "Model 1" }),
          ],
        }),
      ),
    );
  });

  it("does not delete until the user confirms", async () => {
    const user = userEvent.setup();
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    await user.click(
      screen.getByRole("button", { name: "providerCard.action.more" }),
    );
    await user.click(screen.getByRole("menuitem", { name: "common.delete" }));
    expect(copilotByokApi.deleteGroup).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    await waitFor(() =>
      expect(copilotByokApi.deleteGroup).toHaveBeenCalledWith("provider"),
    );
  });

  it("keeps edits visible and reports a failed save", async () => {
    vi.mocked(copilotByokApi.upsertGroup).mockRejectedValueOnce(
      new Error("write failed"),
    );
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    fireEvent.click(
      screen.getByRole("button", { name: "providerCard.action.edit" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("copilotByok.errors.save", {
        description: "write failed",
      }),
    );
    expect(screen.getByLabelText("copilotByok.name")).toHaveValue("Provider");
  });

  it("protects model-only changes when leaving the editor", async () => {
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    fireEvent.click(
      screen.getByRole("button", { name: "providerCard.action.edit" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "copilotByok.addModel" }),
    );
    expect(hasUnsavedChanges()).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "common.back" }));
    expect(screen.getByText("common.unsavedLeaveTitle")).toBeInTheDocument();
    expect(copilotByokApi.upsertGroup).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "common.unsavedLeaveConfirm" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByText("copilotByok.editorTitle"),
      ).not.toBeInTheDocument(),
    );
    expect(hasUnsavedChanges()).toBe(false);
  });

  it("changes device-local targets without selecting a CLI provider", async () => {
    renderWithQueryClient(<CopilotByokSettings mode="targets" />);
    await screen.findByText("/profile/chatLanguageModels.json");
    fireEvent.click(screen.getByRole("checkbox", { name: "Profile" }));
    await waitFor(() =>
      expect(copilotByokApi.setTargets).toHaveBeenCalledWith([]),
    );
  });

  it("does not enable applying models without a selected target", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      targets: [],
      selectedTargetIds: [],
    });
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    expect(
      screen.getByRole("button", { name: "copilotByok.sync" }),
    ).toBeDisabled();
  });

  it("separates added and available providers without claiming a current one", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      groups: [
        state.groups[0],
        { ...state.groups[0], id: "other", name: "Other", enabled: false },
      ],
    });
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    const added = screen.getByRole("region", {
      name: "providerCard.section.added",
    });
    const available = screen.getByRole("region", {
      name: "providerCard.section.available",
    });
    expect(within(added).getByRole("group", { name: "Provider" })).toHaveClass(
      "bg-subtle",
    );
    expect(within(available).getByRole("group", { name: "Other" })).toHaveClass(
      "bg-surface",
    );
    expect(within(added).queryByText("Other")).not.toBeInTheDocument();
    expect(within(available).queryByText("Provider")).not.toBeInTheDocument();
    expect(
      screen.queryByText("providerCard.status.inUse"),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /route|stack/i })).toBeNull();
  });

  it("moves providers between sections using the existing enable/save workflow", async () => {
    vi.mocked(copilotByokApi.upsertGroup).mockImplementation(async (group) => {
      const updated = { ...state, groups: [group] };
      vi.mocked(copilotByokApi.getState).mockResolvedValue(updated);
      return updated;
    });
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    fireEvent.click(
      screen.getByRole("button", { name: "provider.removeFromConfig" }),
    );
    await waitFor(() =>
      expect(
        within(
          screen.getByRole("region", {
            name: "providerCard.section.available",
          }),
        ).getByRole("group", { name: "Provider" }),
      ).toBeInTheDocument(),
    );
    expect(copilotByokApi.upsertGroup).toHaveBeenLastCalledWith({
      ...state.groups[0],
      enabled: false,
    });
    fireEvent.click(
      screen.getByRole("button", { name: "provider.addToConfig" }),
    );
    await waitFor(() =>
      expect(
        within(
          screen.getByRole("region", { name: "providerCard.section.added" }),
        ).getByRole("group", { name: "Provider" }),
      ).toBeInTheDocument(),
    );
    expect(copilotByokApi.upsertGroup).toHaveBeenLastCalledWith(
      state.groups[0],
    );
    expect(copilotByokApi.deleteGroup).not.toHaveBeenCalled();
  });

  it("keeps the group enabled when updating membership fails", async () => {
    vi.mocked(copilotByokApi.upsertGroup).mockRejectedValueOnce(
      new Error("sync failed"),
    );
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    fireEvent.click(
      screen.getByRole("button", { name: "provider.removeFromConfig" }),
    );
    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(
      within(
        screen.getByRole("region", { name: "providerCard.section.added" }),
      ).getByRole("group", { name: "Provider" }),
    ).toBeInTheDocument();
  });

  it("blocks repeated membership changes and editing while a save is pending", async () => {
    let finishSave: (value: CopilotByokState) => void = () => {};
    vi.mocked(copilotByokApi.upsertGroup).mockImplementationOnce(
      () =>
        new Promise<CopilotByokState>((resolve) => {
          finishSave = resolve;
        }),
    );
    renderWithQueryClient(<CopilotByokSettings />);
    await screen.findByText("Provider");
    fireEvent.click(
      screen.getByRole("button", { name: "provider.removeFromConfig" }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "provider.removeFromConfig" }),
      ).toHaveAttribute("aria-disabled", "true"),
    );
    const edit = screen.getByRole("button", {
      name: "providerCard.action.edit",
    });
    expect(edit).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(
      screen.getByRole("button", { name: "provider.removeFromConfig" }),
    );
    fireEvent.click(edit);
    expect(copilotByokApi.upsertGroup).toHaveBeenCalledOnce();
    expect(screen.queryByLabelText("copilotByok.name")).toBeNull();
    await act(async () => finishSave(state));
  });

  it("opens the target page from the local-only notice", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      targets: [],
      selectedTargetIds: [],
    });
    const onOpenTargets = vi.fn();
    renderWithQueryClient(
      <CopilotByokSettings onOpenTargets={onOpenTargets} />,
    );
    await screen.findByText("Provider");
    expect(screen.getByText("copilotByok.noTargets")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "copilotByok.targets" }),
    );
    expect(onOpenTargets).toHaveBeenCalledOnce();
    expect(copilotByokApi.setTargets).not.toHaveBeenCalled();
  });

  it("filters names and URLs with the controlled search, but not credentials", async () => {
    vi.mocked(copilotByokApi.getState).mockResolvedValue({
      ...state,
      groups: [
        state.groups[0],
        {
          ...state.groups[0],
          id: "other",
          name: "Other",
          url: "https://another.test/v1",
          enabled: false,
        },
      ],
    });
    const onSearchOpenChange = vi.fn();
    renderWithQueryClient(
      <CopilotByokSettings
        searchOpen
        onSearchOpenChange={onSearchOpenChange}
      />,
    );
    await screen.findByText("Provider");
    const search = screen.getByRole("textbox", {
      name: "provider.searchAriaLabel",
    });
    fireEvent.change(search, { target: { value: "ANOTHER.TEST" } });
    expect(screen.getByRole("group", { name: "Other" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Provider" })).toBeNull();
    fireEvent.change(search, { target: { value: "provider" } });
    expect(screen.getByRole("group", { name: "Provider" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Other" })).toBeNull();
    fireEvent.change(search, { target: { value: "secret" } });
    expect(screen.getByText("provider.noSearchResults")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.clear" }));
    expect(screen.getAllByRole("group")).toHaveLength(2);
    fireEvent.click(
      screen.getByRole("button", { name: "provider.searchCloseAriaLabel" }),
    );
    expect(onSearchOpenChange).toHaveBeenCalledWith(false);
  });

  it("supports search shortcuts without intercepting editor input", async () => {
    const ref = createRef<CopilotByokSettingsHandle>();
    renderWithQueryClient(<CopilotByokSettings ref={ref} />);
    await screen.findByText("Provider");
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    expect(
      screen.getByRole("textbox", { name: "provider.searchAriaLabel" }),
    ).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(
        screen.queryByRole("textbox", { name: "provider.searchAriaLabel" }),
      ).toBeNull(),
    );
    act(() => ref.current?.openAdd());
    screen.getByLabelText("copilotByok.name").focus();
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    expect(
      screen.queryByRole("textbox", { name: "provider.searchAriaLabel" }),
    ).toBeNull();
  });

  it("keeps target controls independent of provider search", async () => {
    const onSearchOpenChange = vi.fn();
    renderWithQueryClient(
      <CopilotByokSettings
        mode="targets"
        searchOpen
        onSearchOpenChange={onSearchOpenChange}
      />,
    );
    await screen.findByText("/profile/chatLanguageModels.json");
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    expect(onSearchOpenChange).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("textbox", { name: "provider.searchAriaLabel" }),
    ).toBeNull();
    expect(screen.getByRole("checkbox", { name: "Profile" })).toBeEnabled();
  });

  it("focuses a controlled search after the catalog finishes loading", async () => {
    let finishLoad: (value: CopilotByokState) => void = () => {};
    vi.mocked(copilotByokApi.getState).mockImplementationOnce(
      () =>
        new Promise<CopilotByokState>((resolve) => {
          finishLoad = resolve;
        }),
    );
    renderWithQueryClient(<CopilotByokSettings searchOpen />);
    expect(screen.getByText("common.loading")).toBeInTheDocument();
    expect(
      screen.queryByRole("textbox", { name: "provider.searchAriaLabel" }),
    ).toBeNull();
    await act(async () => finishLoad(state));
    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: "provider.searchAriaLabel" }),
      ).toHaveFocus(),
    );
  });

  it("does not display a search-empty message when loading the catalog fails", async () => {
    vi.mocked(copilotByokApi.getState).mockRejectedValueOnce(
      new Error("read failed"),
    );
    renderWithQueryClient(<CopilotByokSettings />);
    expect(
      await screen.findByText("copilotByok.errors.load"),
    ).toBeInTheDocument();
    expect(screen.queryByText("provider.noSearchResults")).toBeNull();
  });

  it("opens only the provider URL and exposes no expansion actions", async () => {
    const user = userEvent.setup();
    const onOpenWebsite = vi.fn();
    renderWithQueryClient(
      <CopilotByokSettings onOpenWebsite={onOpenWebsite} />,
    );
    await screen.findByText("Provider");
    fireEvent.click(
      screen.getByRole("button", { name: "https://example.com" }),
    );
    expect(onOpenWebsite).toHaveBeenCalledWith("https://example.com");
    await user.click(
      screen.getByRole("button", { name: "providerCard.action.more" }),
    );
    expect(
      within(screen.getByRole("menu"))
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["common.delete"]);
    expect(screen.queryByText("secret")).not.toBeInTheDocument();
  });
});
