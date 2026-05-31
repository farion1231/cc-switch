import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import { TagAssignPopover } from "@/components/skills/TagAssignPopover";
import type { SkillTag } from "@/hooks/useSkills";

const setSkillTagsMock = vi.fn();
const createTagMock = vi.fn();
let skillTagsMock: SkillTag[] = [];
let tagAssignmentsMock: [string, number][] = [];

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useSkillTags: () => ({
    data: skillTagsMock,
  }),
  useAllTagAssignments: () => ({
    data: tagAssignmentsMock,
  }),
  useSetSkillTags: () => ({
    mutateAsync: setSkillTagsMock,
  }),
  useCreateTag: () => ({
    mutateAsync: createTagMock,
  }),
}));

describe("TagAssignPopover", () => {
  beforeEach(() => {
    skillTagsMock = [
      { id: 1, name: "密码", sort_index: 0, created_at: 1 },
      { id: 2, name: "写作", sort_index: 1, created_at: 1 },
    ];
    tagAssignmentsMock = [["skill-1", 1]];
    setSkillTagsMock.mockReset();
    createTagMock.mockReset();
  });

  it("renders the assigned tag as selected", () => {
    render(<TagAssignPopover skillId="skill-1" />);

    fireEvent.click(screen.getByTitle("skills.tags.assignTag"));

    const selectedRow = screen.getByText("密码").closest("button");
    expect(selectedRow).toHaveClass("bg-blue-500/10");
  });

  it("replaces the current tag when another tag is selected", async () => {
    setSkillTagsMock.mockResolvedValue(undefined);
    render(<TagAssignPopover skillId="skill-1" />);

    fireEvent.click(screen.getByTitle("skills.tags.assignTag"));
    fireEvent.click(screen.getByText("写作"));

    await waitFor(() => {
      expect(setSkillTagsMock).toHaveBeenCalledWith({
        skillId: "skill-1",
        tagIds: [2],
      });
    });
  });

  it("clears the assignment when the selected tag is clicked", async () => {
    setSkillTagsMock.mockResolvedValue(undefined);
    render(<TagAssignPopover skillId="skill-1" />);

    fireEvent.click(screen.getByTitle("skills.tags.assignTag"));
    fireEvent.click(screen.getByText("密码"));

    await waitFor(() => {
      expect(setSkillTagsMock).toHaveBeenCalledWith({
        skillId: "skill-1",
        tagIds: [],
      });
    });
  });

  it("creates a tag and assigns it to the skill", async () => {
    createTagMock.mockResolvedValue({
      id: 3,
      name: "设计",
      sort_index: 2,
      created_at: 1,
    });
    setSkillTagsMock.mockResolvedValue(undefined);
    render(<TagAssignPopover skillId="skill-1" />);

    fireEvent.click(screen.getByTitle("skills.tags.assignTag"));
    fireEvent.change(
      screen.getByPlaceholderText("skills.tags.tagNamePlaceholder"),
      {
        target: { value: "设计" },
      },
    );
    fireEvent.keyDown(
      screen.getByPlaceholderText("skills.tags.tagNamePlaceholder"),
      {
        key: "Enter",
      },
    );

    await waitFor(() => {
      expect(createTagMock).toHaveBeenCalledWith("设计");
      expect(setSkillTagsMock).toHaveBeenCalledWith({
        skillId: "skill-1",
        tagIds: [3],
      });
    });
  });
});
