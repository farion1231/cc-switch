import { createRef } from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, beforeEach, vi } from "vitest";
import type { ReactElement } from "react";
import {
  SkillsPage,
  type SkillsPageHandle,
} from "@/components/skills/SkillsPage";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastInfoMock = vi.fn();
const toastWarningMock = vi.fn();
const tMock = vi.fn(
  (key: string, options?: Record<string, unknown>) =>
    options ? JSON.stringify({ key, options }) : key,
);

const useDiscoverableSkillsMock = vi.fn();
const useInstalledSkillsMock = vi.fn();
const useSkillReposMock = vi.fn();
const useInstallSkillMock = vi.fn();
const useBatchInstallSkillsMock = vi.fn();
const useAddSkillRepoMock = vi.fn();
const useRemoveSkillRepoMock = vi.fn();
const discoverAvailableMock = vi.fn();
const batchInstallMutateAsyncMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: tMock,
  }),
}));

vi.mock("@/components/skills/SkillCard", () => ({
  SkillCard: ({ skill, selected, onToggleSelect }: any) => (
    <div data-testid={`skill-${skill.key}`}>
      <span>{skill.name}</span>
      {onToggleSelect ? (
        <label>
          <input
            type="checkbox"
            aria-label={`select-${skill.key}`}
            checked={selected}
            onChange={onToggleSelect}
          />
          select
        </label>
      ) : null}
    </div>
  ),
}));

vi.mock("@/components/skills/RepoManagerPanel", () => ({
  RepoManagerPanel: ({ onAdd, onRemove, onClose }: any) => (
    <div data-testid="repo-manager">
      <button
        onClick={() =>
          onAdd({
            owner: "octo",
            name: "skills",
            branch: "main",
            enabled: true,
          })
        }
      >
        add-repo
      </button>
      <button onClick={() => onRemove("octo", "skills")}>remove-repo</button>
      <button onClick={onClose}>close-repo-manager</button>
    </div>
  ),
}));

vi.mock("@/hooks/useSkills", () => ({
  useDiscoverableSkills: (...args: unknown[]) => useDiscoverableSkillsMock(...args),
  useInstalledSkills: (...args: unknown[]) => useInstalledSkillsMock(...args),
  useInstallSkill: (...args: unknown[]) => useInstallSkillMock(...args),
  useBatchInstallSkills: (...args: unknown[]) =>
    useBatchInstallSkillsMock(...args),
  useSkillRepos: (...args: unknown[]) => useSkillReposMock(...args),
  useAddSkillRepo: (...args: unknown[]) => useAddSkillRepoMock(...args),
  useRemoveSkillRepo: (...args: unknown[]) => useRemoveSkillRepoMock(...args),
}));

vi.mock("@/lib/api/skills", async () => {
  const actual = await vi.importActual<object>("@/lib/api/skills");
  return {
    ...actual,
    skillsApi: {
      discoverAvailable: (...args: unknown[]) => discoverAvailableMock(...args),
    },
  };
});

function renderWithQueryClient(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });

  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

describe("SkillsPage", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastInfoMock.mockReset();
    toastWarningMock.mockReset();
    tMock.mockClear();
    discoverAvailableMock.mockReset();

    useDiscoverableSkillsMock.mockReturnValue({
      data: [],
      isLoading: false,
      isFetching: false,
    });
    useInstalledSkillsMock.mockReturnValue({ data: [] });
    useSkillReposMock.mockReturnValue({ data: [], refetch: vi.fn() });
    useInstallSkillMock.mockReturnValue({ mutateAsync: vi.fn() });
    useBatchInstallSkillsMock.mockReturnValue({
      mutateAsync: batchInstallMutateAsyncMock,
      isPending: false,
    });
    useAddSkillRepoMock.mockReturnValue({ mutateAsync: vi.fn() });
    useRemoveSkillRepoMock.mockReturnValue({ mutateAsync: vi.fn() });
    discoverAvailableMock.mockResolvedValue([]);
    batchInstallMutateAsyncMock.mockReset();
  });

  it("forces discover refresh after adding a repo", async () => {
    const ref = createRef<SkillsPageHandle>();
    const addRepoMutateAsync = vi.fn().mockResolvedValue(true);
    useAddSkillRepoMock.mockReturnValue({ mutateAsync: addRepoMutateAsync });
    discoverAvailableMock.mockResolvedValue([
      {
        key: "octo/skills:skill-1",
        name: "Skill One",
        directory: "skill-one",
        description: "",
        repoOwner: "octo",
        repoName: "skills",
        repoBranch: "main",
      },
      {
        key: "octo/skills:skill-2",
        name: "Skill Two",
        directory: "skill-two",
        description: "",
        repoOwner: "octo",
        repoName: "skills",
        repoBranch: "main",
      },
      {
        key: "other/repo:skill-3",
        name: "Skill Three",
        directory: "skill-three",
        description: "",
        repoOwner: "other",
        repoName: "repo",
        repoBranch: "main",
      },
    ]);

    renderWithQueryClient(<SkillsPage ref={ref} />);

    await act(async () => {
      ref.current?.openRepoManager();
    });

    fireEvent.click(await screen.findByText("add-repo"));

    await waitFor(() => {
      expect(addRepoMutateAsync).toHaveBeenCalledWith({
        owner: "octo",
        name: "skills",
        branch: "main",
        enabled: true,
      });
      expect(discoverAvailableMock).toHaveBeenCalledWith(true);
      expect(toastSuccessMock).toHaveBeenCalledWith(
        JSON.stringify({
          key: "skills.repo.addSuccess",
          options: {
            owner: "octo",
            name: "skills",
            count: 2,
          },
        }),
        { closeButton: true },
      );
    });
  });

  it("uses the latest forced discover result to compute added repo count", async () => {
    const ref = createRef<SkillsPageHandle>();
    useAddSkillRepoMock.mockReturnValue({
      mutateAsync: vi.fn().mockResolvedValue(true),
    });
    discoverAvailableMock.mockResolvedValue([
      {
        key: "octo/skills:skill-1",
        name: "Skill One",
        directory: "skill-one",
        description: "",
        repoOwner: "octo",
        repoName: "skills",
        repoBranch: "main",
      },
      {
        key: "octo/skills:skill-2",
        name: "Skill Two",
        directory: "skill-two",
        description: "",
        repoOwner: "octo",
        repoName: "skills",
        repoBranch: "main",
      },
      {
        key: "octo/skills:skill-dev",
        name: "Skill Dev",
        directory: "skill-dev",
        description: "",
        repoOwner: "octo",
        repoName: "skills",
        repoBranch: "dev",
      },
    ]);

    renderWithQueryClient(<SkillsPage ref={ref} />);

    await act(async () => {
      ref.current?.openRepoManager();
    });

    fireEvent.click(await screen.findByText("add-repo"));

    await waitFor(() => {
      expect(toastSuccessMock).toHaveBeenCalledWith(
        JSON.stringify({
          key: "skills.repo.addSuccess",
          options: {
            owner: "octo",
            name: "skills",
            count: 2,
          },
        }),
        { closeButton: true },
      );
    });
  });

  it("forces discover refresh after removing a repo", async () => {
    const ref = createRef<SkillsPageHandle>();
    const removeRepoMutateAsync = vi.fn().mockResolvedValue(true);
    useRemoveSkillRepoMock.mockReturnValue({
      mutateAsync: removeRepoMutateAsync,
    });

    renderWithQueryClient(<SkillsPage ref={ref} />);

    await act(async () => {
      ref.current?.openRepoManager();
    });

    fireEvent.click(await screen.findByText("remove-repo"));

    await waitFor(() => {
      expect(removeRepoMutateAsync).toHaveBeenCalledWith({
        owner: "octo",
        name: "skills",
      });
      expect(discoverAvailableMock).toHaveBeenCalledWith(true);
    });
  });

  it("does not force discover refresh when adding a repo fails", async () => {
    const ref = createRef<SkillsPageHandle>();
    const addRepoMutateAsync = vi
      .fn()
      .mockRejectedValue(new Error("add failed"));
    useAddSkillRepoMock.mockReturnValue({ mutateAsync: addRepoMutateAsync });

    renderWithQueryClient(<SkillsPage ref={ref} />);

    await act(async () => {
      ref.current?.openRepoManager();
    });

    fireEvent.click(await screen.findByText("add-repo"));

    await waitFor(() => {
      expect(addRepoMutateAsync).toHaveBeenCalled();
      expect(toastErrorMock).toHaveBeenCalled();
    });
    expect(discoverAvailableMock).not.toHaveBeenCalled();
  });

  it("batch installs selected uninstalled skills and clears the selection", async () => {
    const discoverableSkills = [
      {
        key: "owner/repo:skill-1",
        name: "Skill One",
        directory: "skill-one",
        description: "",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
      },
      {
        key: "owner/repo:skill-2",
        name: "Skill Two",
        directory: "skill-two",
        description: "",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
      },
    ];
    useDiscoverableSkillsMock.mockReturnValue({
      data: discoverableSkills,
      isLoading: false,
      isFetching: false,
    });
    batchInstallMutateAsyncMock.mockResolvedValue({
      installed: [
        {
          id: "owner/repo:skill-1",
          name: "Skill One",
          directory: "skill-one",
          apps: { claude: true, codex: false, gemini: false, opencode: false },
          installedAt: 1,
        },
        {
          id: "owner/repo:skill-2",
          name: "Skill Two",
          directory: "skill-two",
          apps: { claude: true, codex: false, gemini: false, opencode: false },
          installedAt: 2,
        },
      ],
      failed: [],
    });

    renderWithQueryClient(<SkillsPage />);

    fireEvent.click(screen.getByLabelText("select-owner/repo:skill-1"));
    fireEvent.click(screen.getByLabelText("select-owner/repo:skill-2"));
    fireEvent.click(screen.getByRole("button", { name: /skills\.batchInstall|Install Selected|安装选中/ }));

    await waitFor(() => {
      expect(batchInstallMutateAsyncMock).toHaveBeenCalledWith({
        skills: discoverableSkills.map((skill) => ({
          ...skill,
          installed: false,
        })),
        currentApp: "claude",
      });
    });

    await waitFor(() => {
      expect(
        screen.getByLabelText("select-owner/repo:skill-1"),
      ).not.toBeChecked();
      expect(
        screen.getByLabelText("select-owner/repo:skill-2"),
      ).not.toBeChecked();
    });
  });

  it("shows a warning when batch install partially fails", async () => {
    const discoverableSkills = [
      {
        key: "owner/repo:skill-1",
        name: "Skill One",
        directory: "skill-one",
        description: "",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
      },
      {
        key: "owner/repo:skill-2",
        name: "Skill Two",
        directory: "skill-two",
        description: "",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
      },
    ];
    useDiscoverableSkillsMock.mockReturnValue({
      data: discoverableSkills,
      isLoading: false,
      isFetching: false,
    });
    batchInstallMutateAsyncMock.mockResolvedValue({
      installed: [
        {
          id: "owner/repo:skill-1",
          name: "Skill One",
          directory: "skill-one",
          apps: { claude: true, codex: false, gemini: false, opencode: false },
          installedAt: 1,
        },
      ],
      failed: [{ key: "owner/repo:skill-2", error: "install failed" }],
    });

    renderWithQueryClient(<SkillsPage />);

    fireEvent.click(screen.getByLabelText("select-owner/repo:skill-1"));
    fireEvent.click(screen.getByLabelText("select-owner/repo:skill-2"));
    fireEvent.click(screen.getByRole("button", { name: /skills\.batchInstall|Install Selected|安装选中/ }));

    await waitFor(() => {
      expect(toastWarningMock).toHaveBeenCalled();
    });
  });
});
