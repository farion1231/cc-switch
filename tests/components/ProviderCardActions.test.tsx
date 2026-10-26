import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderCardActions } from "@/components/providers/ProviderCardActions";
import type { CardPresentation } from "@/components/providers/presentation";

async function openMenu(presentation: CardPresentation) {
  const user = userEvent.setup();
  render(
    <ProviderCardActions
      providerName="Kimi"
      presentation={presentation}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onDuplicate={vi.fn()}
    />,
  );
  await user.click(
    screen.getByRole("button", { name: "providerCard.action.more" }),
  );
  return { user, menu: await screen.findByRole("menu") };
}

describe("ProviderCardActions — mode items in the more menu", () => {
  it("lists them first", async () => {
    const onSelect = vi.fn();
    const { user, menu } = await openMenu({
      buttons: [],
      chips: [],
      menuItems: [
        {
          key: "setDefault",
          label: "providerCard.action.setDefault",
          onSelect,
        },
      ],
    });

    const items = within(menu).getAllByRole("menuitem");
    expect(items[0]).toHaveTextContent("providerCard.action.setDefault");
    expect(items[1]).toHaveTextContent("provider.duplicate");

    await user.click(items[0]);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("keeps a disabled one listed with its reason", async () => {
    const onSelect = vi.fn();
    const { user, menu } = await openMenu({
      buttons: [],
      chips: [],
      menuItems: [
        {
          key: "setDefault",
          label: "providerCard.action.setDefault",
          disabledReason: "providerCard.reason.defaultWhileRouting",
          onSelect,
        },
      ],
    });

    const item = within(menu).getAllByRole("menuitem")[0];
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveTextContent("providerCard.reason.defaultWhileRouting");
    await user.click(item);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("opens a submenu to pick the default model", async () => {
    const onSelect = vi.fn();
    const pick = vi.fn();
    const { user, menu } = await openMenu({
      buttons: [],
      chips: [],
      menuItems: [
        {
          key: "setDefault",
          label: "providerCard.action.setDefault",
          onSelect,
          submenu: {
            title: "openclaw.selectDefaultModel",
            options: [
              {
                key: "m-1",
                label: "Model 1",
                detail: "m-1",
                onSelect: vi.fn(),
              },
              { key: "m-2", label: "m-2", onSelect: pick },
            ],
          },
        },
      ],
    });

    const trigger = within(menu).getAllByRole("menuitem")[0];
    expect(trigger).toHaveTextContent("providerCard.action.setDefault");
    await user.click(trigger);
    const submenu = (await screen.findAllByRole("menu")).at(-1)!;
    expect(submenu).toHaveTextContent("openclaw.selectDefaultModel");
    // jsdom 里点子菜单项不会触发选择，改用键盘选
    fireEvent.keyDown(within(submenu).getByRole("menuitem", { name: "m-2" }), {
      key: "Enter",
    });
    expect(pick).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("shows only the usual items when the mode has none", async () => {
    const { menu } = await openMenu({ buttons: [], chips: [] });

    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["provider.duplicate", "common.delete"]);
  });
});
