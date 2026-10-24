import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  afterAll,
  beforeAll,
  describe,
  expect,
  it,
  vi,
  beforeEach,
} from "vitest";

import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";
import type {
  DiscoverableSkill,
  GitCredential,
  SkillRepo,
} from "@/lib/api/skills";

const m = vi.hoisted(() => ({
  createCredential: vi.fn(),
  deleteCredential: vi.fn(),
  openExternal: vi.fn(),
  credentials: [] as GitCredential[],
  credentialsPending: false,
  credentialsError: false,
}));

vi.mock("@/lib/api", () => ({
  settingsApi: { openExternal: m.openExternal },
}));

vi.mock("@/hooks/useSkills", () => ({
  useGitCredentials: () => ({
    data: m.credentials,
    isPending: m.credentialsPending,
    isError: m.credentialsError,
    refetch: vi.fn(),
  }),
  useCreateGitCredential: () => ({
    mutateAsync: m.createCredential,
    isPending: false,
  }),
  useDeleteGitCredential: () => ({
    mutateAsync: m.deleteCredential,
    isPending: false,
  }),
}));

const makeCredential = (
  overrides: Partial<GitCredential> = {},
): GitCredential => ({
  id: "c1",
  host: "github.com",
  name: "工作账号",
  createdAt: 1,
  updatedAt: 1,
  ...overrides,
});

const makeRepo = (overrides: Partial<SkillRepo> = {}): SkillRepo => ({
  owner: "owner-a",
  name: "repo-a",
  branch: "main",
  enabled: true,
  private: false,
  credentialId: null,
  ...overrides,
});

function renderPanel(
  overrides: Partial<React.ComponentProps<typeof RepoManagerPanel>> = {},
) {
  const props = {
    repos: [] as SkillRepo[],
    skills: [] as DiscoverableSkill[],
    onAdd: vi.fn().mockResolvedValue(undefined),
    onRemove: vi.fn().mockResolvedValue(undefined),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<RepoManagerPanel {...props} />);
  return props;
}

/** i18n 在测试里没有资源，t() 原样返回 key，所以断言用 key 而不是文案。 */
const typeRepo = (url: string) =>
  userEvent.type(screen.getByLabelText("skills.repo.url"), url);

const clickAdd = () =>
  userEvent.click(screen.getByRole("button", { name: /skills\.repo\.add/ }));

const tickPrivate = () =>
  userEvent.click(screen.getByLabelText("skills.repo.private"));

const openTokenPicker = () =>
  userEvent.click(
    screen.getByRole("combobox", { name: "skills.credential.label" }),
  );

// Radix Select 会滚动到聚焦项；jsdom 没有实现滚动。
const originalScrollIntoView = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  "scrollIntoView",
);
beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
});
afterAll(() => {
  if (originalScrollIntoView) {
    Object.defineProperty(
      HTMLElement.prototype,
      "scrollIntoView",
      originalScrollIntoView,
    );
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  }
});

beforeEach(() => {
  m.credentials = [];
  m.credentialsPending = false;
  m.credentialsError = false;
  m.createCredential
    .mockReset()
    .mockResolvedValue(makeCredential({ id: "new" }));
  m.deleteCredential.mockReset().mockResolvedValue([]);
  m.openExternal.mockReset().mockResolvedValue(undefined);
});

describe("RepoManagerPanel", () => {
  it("adds a public repository without any token", async () => {
    const props = renderPanel();

    await typeRepo("https://github.com/acme/skills");
    await clickAdd();

    await waitFor(() =>
      expect(props.onAdd).toHaveBeenCalledWith({
        owner: "acme",
        name: "skills",
        branch: "main",
        enabled: true,
        private: false,
        credentialId: null,
      }),
    );
  });

  it("hides the token picker until the repository is marked private", async () => {
    renderPanel();

    expect(
      screen.queryByRole("combobox", { name: "skills.credential.label" }),
    ).not.toBeInTheDocument();

    await tickPrivate();

    expect(
      screen.getByRole("combobox", { name: "skills.credential.label" }),
    ).toBeInTheDocument();
  });

  it("explains the token requirements behind the help button", async () => {
    renderPanel();
    await tickPrivate();

    expect(
      screen.getByRole("button", { name: "skills.credential.helpTitle" }),
    ).toBeInTheDocument();
  });

  it("points at adding a token when none exist yet", async () => {
    renderPanel();
    await tickPrivate();

    expect(screen.getByText("skills.credential.empty")).toBeInTheDocument();
  });

  it("adds a private repository with the selected token", async () => {
    m.credentials = [makeCredential({ id: "c1" })];
    const props = renderPanel();

    await typeRepo("acme/skills");
    await tickPrivate();
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: "工作账号" }),
    );
    await clickAdd();

    await waitFor(() =>
      expect(props.onAdd).toHaveBeenCalledWith(
        expect.objectContaining({ private: true, credentialId: "c1" }),
      ),
    );
  });

  it("forgets the previous repository settings once the url stops matching", async () => {
    m.credentials = [makeCredential()];
    const props = renderPanel({
      repos: [
        makeRepo({
          owner: "acme",
          name: "private-tools",
          branch: "develop",
          private: true,
          credentialId: "c1",
        }),
      ],
    });

    await typeRepo("acme/private-tools");
    expect(screen.getByLabelText("skills.repo.private")).toBeChecked();
    expect(screen.getByLabelText("skills.repo.branch")).toHaveValue("develop");

    // 改成一个没见过的仓库：上一个仓库的私有勾选、分支和令牌都必须跟着失效，
    // 否则新仓库会被静默建成私有并绑上别人的令牌
    await userEvent.clear(screen.getByLabelText("skills.repo.url"));
    await typeRepo("acme/new-tools");
    expect(screen.getByLabelText("skills.repo.private")).not.toBeChecked();
    expect(screen.getByLabelText("skills.repo.branch")).toHaveValue("");

    await clickAdd();
    await waitFor(() => expect(props.onAdd).toHaveBeenCalled());
    expect(props.onAdd).toHaveBeenCalledWith({
      owner: "acme",
      name: "new-tools",
      branch: "main",
      enabled: true,
      private: false,
      credentialId: null,
    });
  });

  it("opens the dialog from the picker without changing the current choice", async () => {
    m.credentials = [makeCredential({ id: "c1" })];
    renderPanel();
    await tickPrivate();

    const picker = () =>
      screen.getByRole("combobox", { name: "skills.credential.label" });

    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: "工作账号" }),
    );
    expect(picker()).toHaveTextContent("工作账号");

    // 「添加令牌」不是一次选择，只是打开对话框
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: /skills\.credential\.add/ }),
    );
    expect(
      await screen.findByText("skills.credential.addTitle"),
    ).toBeInTheDocument();

    // 关掉对话框再看：当前选择没有被「添加令牌」改掉
    // （对话框打开时 Radix 会把外部内容移出无障碍树，这时查不到下拉框）
    await userEvent.click(
      screen.getByRole("button", { name: "skills.credential.cancel" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByText("skills.credential.addTitle"),
      ).not.toBeInTheDocument(),
    );
    expect(picker()).toHaveTextContent("工作账号");
  });

  it("requires both a name and a token before saving", async () => {
    renderPanel();
    await tickPrivate();
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: /skills\.credential\.add/ }),
    );

    const save = screen.getByRole("button", { name: "skills.credential.save" });
    expect(save).toBeDisabled();

    await userEvent.type(
      screen.getByLabelText("skills.credential.name"),
      "工作账号",
    );
    expect(save).toBeDisabled();

    await userEvent.type(
      screen.getByLabelText("skills.credential.token"),
      "github_pat_abc",
    );
    expect(save).toBeEnabled();
  });

  it("creates a token and selects it", async () => {
    const props = renderPanel();
    await tickPrivate();
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: /skills\.credential\.add/ }),
    );

    await userEvent.type(
      screen.getByLabelText("skills.credential.name"),
      "工作账号",
    );
    await userEvent.type(
      screen.getByLabelText("skills.credential.token"),
      "github_pat_abc",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "skills.credential.save" }),
    );

    await waitFor(() =>
      expect(m.createCredential).toHaveBeenCalledWith({
        name: "工作账号",
        token: "github_pat_abc",
      }),
    );

    // 新建之后要真的选中它：否则用户以为选了，提交时却是空的
    await typeRepo("acme/skills");
    await clickAdd();
    await waitFor(() =>
      expect(props.onAdd).toHaveBeenCalledWith(
        expect.objectContaining({ private: true, credentialId: "new" }),
      ),
    );
  });

  it("reveals the token in plain text on request", async () => {
    renderPanel();
    await tickPrivate();
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: /skills\.credential\.add/ }),
    );

    const token = screen.getByLabelText("skills.credential.token");
    await userEvent.type(token, "github_pat_abc");
    expect(token).toHaveAttribute("type", "password");

    await userEvent.click(
      screen.getByRole("button", { name: "skills.credential.show" }),
    );
    expect(token).toHaveAttribute("type", "text");
    // 明文状态下按钮改成「隐藏」
    expect(
      screen.getByRole("button", { name: "skills.credential.hide" }),
    ).toBeInTheDocument();
  });

  it("does not claim there are no tokens while the list is still loading", async () => {
    m.credentialsPending = true;
    renderPanel();
    await tickPrivate();

    expect(
      screen.queryByText("skills.credential.empty"),
    ).not.toBeInTheDocument();
  });

  it("reports a failed token list instead of pretending there are none", async () => {
    m.credentialsError = true;
    renderPanel();
    await tickPrivate();

    expect(
      screen.getByText("skills.credential.loadFailed"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("skills.credential.empty"),
    ).not.toBeInTheDocument();
  });

  it("announces a failed token save", async () => {
    m.createCredential.mockRejectedValue(
      JSON.stringify({
        code: "CREDENTIAL_INVALID",
        context: { host: "github.com" },
        suggestion: "reenterCredential",
      }),
    );
    renderPanel();
    await tickPrivate();
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: /skills\.credential\.add/ }),
    );

    await userEvent.type(
      screen.getByLabelText("skills.credential.name"),
      "工作账号",
    );
    await userEvent.type(
      screen.getByLabelText("skills.credential.token"),
      "ghp_bad",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "skills.credential.save" }),
    );

    // 提交失败要进无障碍树，而不是只有视觉上的红字
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("skills.error.credentialInvalid");
    expect(screen.getByLabelText("skills.credential.token")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });

  it("surfaces a structured failure when saving a token fails", async () => {
    m.createCredential.mockRejectedValue(
      JSON.stringify({
        code: "KEYRING_UNAVAILABLE",
        context: { host: "github.com" },
        suggestion: "checkKeyring",
      }),
    );
    renderPanel();
    await tickPrivate();
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: /skills\.credential\.add/ }),
    );

    await userEvent.type(
      screen.getByLabelText("skills.credential.name"),
      "工作账号",
    );
    await userEvent.type(
      screen.getByLabelText("skills.credential.token"),
      "github_pat_abc",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "skills.credential.save" }),
    );

    expect(
      await screen.findByText(/skills\.error\.keyringUnavailable/),
    ).toBeInTheDocument();
  });

  it("removes the selected token only after confirmation", async () => {
    m.credentials = [makeCredential({ id: "c1", name: "工作账号" })];
    renderPanel();
    await tickPrivate();
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: "工作账号" }),
    );

    await userEvent.click(
      screen.getByRole("button", { name: "skills.credential.remove" }),
    );
    // 确认框先出现，此时还没删
    expect(
      await screen.findByText("skills.credential.removeTitle"),
    ).toBeInTheDocument();
    expect(m.deleteCredential).not.toHaveBeenCalled();

    await userEvent.click(
      screen.getByRole("button", { name: "skills.credential.removeConfirm" }),
    );
    await waitFor(() => expect(m.deleteCredential).toHaveBeenCalledWith("c1"));
  });

  it("rejects a malformed repository url without calling the backend", async () => {
    const props = renderPanel();

    await typeRepo("https://github.com/only-owner");
    await clickAdd();

    expect(
      await screen.findByText("skills.repo.invalidUrl"),
    ).toBeInTheDocument();
    expect(props.onAdd).not.toHaveBeenCalled();
  });

  it("flags private repositories in the list", () => {
    renderPanel({ repos: [makeRepo({ private: true })] });

    expect(screen.getByText(/skills\.repo\.privateBadge/)).toBeInTheDocument();
  });

  it("reports a structured failure from the private-repo preflight", async () => {
    const onAdd = vi.fn().mockRejectedValue(
      JSON.stringify({
        code: "AUTH_REQUIRED",
        context: { owner: "acme", name: "skills" },
        suggestion: "configureCredential",
      }),
    );
    renderPanel({ onAdd });

    await typeRepo("acme/skills");
    await tickPrivate();
    await clickAdd();

    // 不能把后端那串 JSON 原样丢给用户
    const shown = await screen.findByText(/skills\.error\.authRequired/);
    expect(shown).toBeInTheDocument();
    expect(shown.textContent).toContain(
      "skills.error.suggestion.configureCredential",
    );
    // 这次失败就是令牌的问题，下拉框要一起进错误态
    expect(
      screen.getByRole("combobox", { name: "skills.credential.label" }),
    ).toHaveAttribute("aria-invalid", "true");
  });

  it("does not blame the token picker for a failure that is not about the token", async () => {
    m.credentials = [makeCredential()];
    const onAdd = vi.fn().mockRejectedValue(
      JSON.stringify({
        code: "INVALID_REPO_REF",
        context: { owner: "acme", name: "skills" },
        suggestion: "checkRepoUrl",
      }),
    );
    renderPanel({ onAdd });

    await typeRepo("acme/skills");
    await tickPrivate();
    await openTokenPicker();
    await userEvent.click(
      await screen.findByRole("option", { name: "工作账号" }),
    );
    await clickAdd();

    await screen.findByText(/skills\.error\.invalidRepoRef/);
    expect(
      screen.getByRole("combobox", { name: "skills.credential.label" }),
    ).not.toHaveAttribute("aria-invalid", "true");
  });
});
