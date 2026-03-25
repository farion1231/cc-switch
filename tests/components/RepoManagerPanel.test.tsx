import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";

vi.mock("@/lib/api", () => ({
  settingsApi: {
    openExternal: vi.fn(),
  },
}));

describe("RepoManagerPanel", () => {
  it("accepts GitLab repo URLs and passes repo metadata", async () => {
    const onAdd = vi.fn().mockResolvedValue(undefined);

    render(
      <RepoManagerPanel
        repos={[]}
        skills={[]}
        onAdd={onAdd}
        onRemove={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    fireEvent.change(screen.getByLabelText("skills.repo.url"), {
      target: { value: "https://gitlabwh.uniontech.com/ut001335/uos-skills" },
    });
    fireEvent.change(screen.getByLabelText("skills.repo.branch"), {
      target: { value: "main" },
    });
    fireEvent.click(screen.getByText("skills.repo.add"));

    await waitFor(() => {
      expect(onAdd).toHaveBeenCalledWith({
        owner: "gitlabwh.uniontech.com/ut001335",
        name: "uos-skills",
        branch: "main",
        enabled: true,
        provider: "gitlab",
        repoUrl: "https://gitlabwh.uniontech.com/ut001335/uos-skills.git",
      });
    });
  });
});
