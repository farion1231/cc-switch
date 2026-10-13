import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import {
  SkillCategoryManager,
  SkillCategoryPicker,
} from "@/components/skills/SkillCategoryDialogs";
import type { InstalledSkill } from "@/lib/api/skills";
import zh from "@/i18n/locales/zh.json";

async function localizedRender(children: React.ReactNode) {
  const i18n = createInstance();
  await i18n.init({
    lng: "zh",
    resources: { zh: { translation: zh } },
    interpolation: { escapeValue: false },
  });
  return render(<I18nextProvider i18n={i18n}>{children}</I18nextProvider>);
}

describe("Skill category dialogs", () => {
  it("shows the actual affected count and keeps failed deletion open", async () => {
    const onAction = vi.fn().mockResolvedValue(false);
    const categories = [{ id: "dev", name: "开发" }];
    const skills = [
      { id: "a", categoryId: "dev" },
      { id: "b", categoryId: "dev" },
      { id: "c", categoryId: null },
    ] as InstalledSkill[];
    await localizedRender(
      <SkillCategoryManager
        categories={categories}
        skills={skills}
        pending={false}
        onAction={onAction}
        onClose={vi.fn()}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "删除 开发" }));
    const confirm = screen
      .getByText(/2.*Skill/)
      .closest<HTMLElement>('[role="dialog"]')!;
    expect(confirm).toBeInTheDocument();
    await userEvent.click(
      within(confirm).getByRole("button", { name: "删除" }),
    );
    await waitFor(() =>
      expect(onAction).toHaveBeenCalledWith({ kind: "delete", id: "dev" }),
    );
    expect(confirm).toBeInTheDocument();
  });

  it("retains the selected category after assignment fails so it can be retried", async () => {
    const onAssign = vi
      .fn()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const onClose = vi.fn();
    await localizedRender(
      <SkillCategoryPicker
        categories={[{ id: "dev", name: "开发" }]}
        currentId={null}
        count={2}
        pending={false}
        onAssign={onAssign}
        onClose={onClose}
      />,
    );
    await userEvent.selectOptions(screen.getByRole("combobox"), "dev");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(onAssign).toHaveBeenCalledWith("dev"));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("combobox")).toHaveValue("dev");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  });
});
