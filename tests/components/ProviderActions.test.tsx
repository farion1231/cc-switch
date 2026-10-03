import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderActions } from "@/components/providers/ProviderActions";

function renderPiActions({
  isCurrent = false,
  isInConfig = false,
  isRemovalProtected = false,
  isStateChangeProtected = false,
  isAutoFailoverEnabled = false,
  isInFailoverQueue = false,
  onSwitch = vi.fn(),
  onEdit = vi.fn(),
  onRemoveFromConfig = vi.fn(),
  onDelete = vi.fn(),
  onSetAsDefault = vi.fn(),
  onToggleFailover,
}: {
  isCurrent?: boolean;
  isInConfig?: boolean;
  isRemovalProtected?: boolean;
  isStateChangeProtected?: boolean;
  isAutoFailoverEnabled?: boolean;
  isInFailoverQueue?: boolean;
  onSwitch?: ReturnType<typeof vi.fn>;
  onEdit?: ReturnType<typeof vi.fn>;
  onRemoveFromConfig?: ReturnType<typeof vi.fn>;
  onDelete?: ReturnType<typeof vi.fn>;
  onSetAsDefault?: ReturnType<typeof vi.fn>;
  onToggleFailover?: ReturnType<typeof vi.fn>;
}) {
  render(
    <ProviderActions
      appId="pi"
      isCurrent={isCurrent}
      isInConfig={isInConfig}
      isRemovalProtected={isRemovalProtected}
      isStateChangeProtected={isStateChangeProtected}
      isAutoFailoverEnabled={isAutoFailoverEnabled}
      isInFailoverQueue={isInFailoverQueue}
      onToggleFailover={onToggleFailover}
      onSwitch={onSwitch}
      onRemoveFromConfig={onRemoveFromConfig}
      onSetAsDefault={onSetAsDefault}
      onEdit={onEdit}
      onDuplicate={vi.fn()}
      onDelete={onDelete}
    />,
  );
  return { onSwitch, onEdit, onRemoveFromConfig, onDelete, onSetAsDefault };
}

describe("ProviderActions Pi provider switching", () => {
  it("omits duplication when the caller disallows it", () => {
    render(
      <ProviderActions
        appId="codex"
        isCurrent={false}
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.queryByTitle("provider.duplicate")).not.toBeInTheDocument();
  });

  it("enables a provider that is not in Pi", async () => {
    const user = userEvent.setup();
    const { onSwitch } = renderPiActions({});

    await user.click(screen.getByRole("button", { name: "启用" }));

    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("button", { name: "provider.setAsDefault" }),
    ).not.toBeInTheDocument();
  });

  it("offers removal without a default-selection action", async () => {
    const user = userEvent.setup();
    const { onRemoveFromConfig, onSetAsDefault, onSwitch } = renderPiActions({
      isInConfig: true,
    });

    await user.click(screen.getByRole("button", { name: "移除" }));

    expect(onRemoveFromConfig).toHaveBeenCalledTimes(1);
    expect(onSetAsDefault).not.toHaveBeenCalled();
    expect(onSwitch).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "设为默认" }),
    ).not.toBeInTheDocument();
  });

  it("does not turn Pi's current selection into a UI state", () => {
    renderPiActions({
      isCurrent: true,
      isInConfig: true,
    });

    expect(screen.getByRole("button", { name: "移除" })).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "当前默认" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "common.delete" })).toBeEnabled();
  });

  it("fails closed while Pi's authoritative state is unavailable", async () => {
    const user = userEvent.setup();
    const { onSwitch, onEdit, onDelete } = renderPiActions({
      isStateChangeProtected: true,
    });

    const enableButton = screen.getByRole("button", { name: "启用" });
    const deleteButton = screen.getByRole("button", {
      name: "common.delete",
    });
    const editButton = screen.getByRole("button", { name: "common.edit" });
    expect(enableButton).toBeDisabled();
    expect(deleteButton).toBeDisabled();
    expect(editButton).toBeEnabled();

    await user.click(enableButton);
    await user.click(editButton);
    await user.click(deleteButton);
    expect(onSwitch).not.toHaveBeenCalled();
    expect(onEdit).toHaveBeenCalledTimes(1);
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("keeps Pi in membership mode even if stale failover props are supplied", async () => {
    const user = userEvent.setup();
    const onToggleFailover = vi.fn();
    const { onSwitch } = renderPiActions({
      isAutoFailoverEnabled: true,
      isInFailoverQueue: false,
      onToggleFailover,
    });

    await user.click(screen.getByRole("button", { name: "启用" }));

    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(onToggleFailover).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "failover.addQueue" }),
    ).not.toBeInTheDocument();
  });
});

describe("ProviderActions Stack mode", () => {
  const renderStack = (
    props: Partial<Parameters<typeof ProviderActions>[0]> = {},
  ) =>
    render(
      <ProviderActions
        appId="claude"
        isCurrent={false}
        isProxyTakeover
        isStackMode
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        {...props}
      />,
    );

  it("adds a provider that is not in the list", async () => {
    const user = userEvent.setup();
    const onToggleStack = vi.fn();
    renderStack({ onToggleStack });

    await user.click(screen.getByRole("button", { name: "添加" }));
    expect(onToggleStack).toHaveBeenLastCalledWith(true);
    // 没添加的不能设为默认。
    expect(screen.queryByRole("button", { name: "设为默认" })).toBeNull();
  });

  it("removes an added provider and sets it as the default", async () => {
    const user = userEvent.setup();
    const onToggleStack = vi.fn();
    const onSwitch = vi.fn();
    renderStack({ isStackMember: true, onToggleStack, onSwitch });

    await user.click(screen.getByRole("button", { name: "设为默认" }));
    expect(onSwitch).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "移除" }));
    expect(onToggleStack).toHaveBeenLastCalledWith(false);
  });

  it("keeps the default in the list", () => {
    renderStack({ isCurrent: true, isStackMember: true, onToggleStack: vi.fn() });

    expect(screen.getByRole("button", { name: "移除" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "当前默认" })).toBeDisabled();
  });

  it("only lets an account that cannot be added become the default", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    renderStack({ appId: "codex", onSwitch });

    expect(screen.queryByRole("button", { name: "添加" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "设为默认" }));
    expect(onSwitch).toHaveBeenCalledTimes(1);
  });

  it("keeps an official account blocked by the proxy blocked", () => {
    renderStack({ isOfficialBlockedByProxy: true });

    expect(screen.queryByRole("button", { name: "设为默认" })).toBeNull();
  });
});
