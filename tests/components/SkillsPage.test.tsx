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

const useDiscoverableSkillsMock = vi.fn();
const useInstalledSkillsMock = vi.fn();
const useSkillReposMock = vi.fn();
const useInstallSkillMock = vi.fn();
const useBatchInstallSkillsMock = vi.fn();
const useAddSkillRepoMock = vi.fn();
const useRemoveSkillRepoMock = vi.fn();
const discoverAvailableMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
  },
}));

vi.mock("@/components/skills/SkillCard", () => ({
  SkillCard: ({ skill }: any) => <div data-testid={`skill-${skill.key}`} />,
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
      mutateAsync: vi.fn(),
      isPending: false,
    });
    useAddSkillRepoMock.mockReturnValue({ mutateAsync: vi.fn() });
    useRemoveSkillRepoMock.mockReturnValue({ mutateAsync: vi.fn() });
    discoverAvailableMock.mockResolvedValue([]);
  });

  it("forces discover refresh after adding a repo", async () => {
    const ref = createRef<SkillsPageHandle>();
    const addRepoMutateAsync = vi.fn().mockResolvedValue(true);
    useAddSkillRepoMock.mockReturnValue({ mutateAsync: addRepoMutateAsync });

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
});
