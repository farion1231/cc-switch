import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SkillImportDialog } from "@/components/skills/SkillImportDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("Pi-family native Skills import", () => {
  it("displays native origins but never offers or enables Pi/rpi flags", () => {
    const onImport = vi.fn();
    render(
      <SkillImportDialog
        skills={[
          {
            directory: "native",
            name: "Native skill",
            path: "/isolated/native",
            foundIn: ["pi", "rpi", "claude"],
          },
        ]}
        visibleAppIds={["claude", "pi", "rpi"]}
        isImporting={false}
        onImport={onImport}
        onClose={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole("checkbox", { name: /^Pi$/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", { name: /^rpi$/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: /^Claude Code$/ }),
    ).toBeChecked();
    fireEvent.click(
      screen.getByRole("button", { name: "skillsPage.import.submit" }),
    );
    expect(onImport).toHaveBeenCalledWith([
      expect.objectContaining({
        directory: "native",
        apps: expect.objectContaining({ claude: true, pi: false, rpi: false }),
      }),
    ]);
  });
});
