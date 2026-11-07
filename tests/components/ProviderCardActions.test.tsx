import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProviderCardActions } from "@/components/providers/ProviderCardActions";
import { useIconOnlyActions } from "@/components/providers/useIconOnlyActions";
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

describe("ProviderCardActions — open terminal", () => {
  it("is a button on the card, not an item in the more menu", async () => {
    const user = userEvent.setup();
    const onOpenTerminal = vi.fn();
    render(
      <ProviderCardActions
        providerName="Claude Official"
        presentation={{ buttons: [], chips: [] }}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenTerminal={onOpenTerminal}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "providerCard.action.openTerminal" }),
    );
    expect(onOpenTerminal).toHaveBeenCalledTimes(1);

    await user.click(
      screen.getByRole("button", { name: "providerCard.action.more" }),
    );
    const menu = await screen.findByRole("menu");
    expect(
      within(menu).queryByRole("menuitem", { name: "provider.openTerminal" }),
    ).not.toBeInTheDocument();
  });

  it("isn't shown for apps without it", () => {
    render(
      <ProviderCardActions
        providerName="Kimi"
        presentation={{ buttons: [], chips: [] }}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole("button", {
        name: "providerCard.action.openTerminal",
      }),
    ).not.toBeInTheDocument();
  });
});

describe("ProviderCardActions — primary button", () => {
  const presentation: CardPresentation = {
    buttons: [{ key: "switch", label: "provider.switch", onClick: vi.fn() }],
    chips: [],
  };

  it("shows its name on the button when there is room", () => {
    render(
      <ProviderCardActions
        providerName="Kimi"
        presentation={presentation}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: "provider.switch" }),
    ).toHaveTextContent("provider.switch");
  });

  it("falls back to an icon with an accessible name when the card is cramped", () => {
    render(
      <ProviderCardActions
        providerName="Kimi"
        presentation={presentation}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        iconOnly
      />,
    );
    const button = screen.getByRole("button", { name: "provider.switch" });
    expect(button).toHaveAttribute("aria-label", "provider.switch");
    expect(button.textContent).toBe("");
  });
});

describe("useIconOnlyActions", () => {
  // 名字列的宽度 = 外部给的可用宽度 + 收成图标时按钮文字让出来的 100px
  const FREED = 100;
  let available = 300;
  let observed: (() => void) | undefined;

  function Harness({ label = "Switch" }: { label?: string }) {
    const ref = useRef<HTMLDivElement>(null);
    const iconOnly = useIconOnlyActions(ref, label);
    return (
      <div
        ref={(el) => {
          (ref as { current: HTMLDivElement | null }).current = el;
          if (el) {
            el.getBoundingClientRect = () =>
              ({
                width: available + (el.dataset.iconOnly === "true" ? FREED : 0),
              }) as DOMRect;
          }
        }}
        data-testid="name"
        data-icon-only={iconOnly}
      />
    );
  }

  const resize = (width: number) => {
    available = width;
    act(() => observed?.());
  };
  const iconOnly = () =>
    screen.getByTestId("name").getAttribute("data-icon-only") === "true";

  afterEach(() => {
    vi.unstubAllGlobals();
    available = 300;
    observed = undefined;
  });

  function setup(label?: string) {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          observed = callback;
        }
        observe() {
          observed?.();
        }
        disconnect() {}
      },
    );
    return render(<Harness label={label} />);
  }

  it("keeps the name when the name column has room", () => {
    setup();
    expect(iconOnly()).toBe(false);
  });

  it("switches to icons once the name column gets too narrow", () => {
    setup();
    resize(80);
    expect(iconOnly()).toBe(true);
  });

  // 收起后名字列多出 100px；只要扣掉这 100px 还不够宽就别放回文字，免得在临界宽度来回跳
  it("doesn't flip back until the name column can give the label its room again", () => {
    setup();
    resize(80);
    resize(100);
    expect(iconOnly()).toBe(true);
    resize(110);
    expect(iconOnly()).toBe(false);
  });

  it("re-measures with the name when the button label changes", () => {
    const { rerender } = setup();
    resize(80);
    expect(iconOnly()).toBe(true);
    available = 300;
    rerender(<Harness label="Remove from queue" />);
    expect(iconOnly()).toBe(false);
  });
});
