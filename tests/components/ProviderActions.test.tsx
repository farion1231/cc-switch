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

function renderDshActions({
  isCurrent = false,
  isInConfig = false,
  isDefaultModel = false,
  isReadOnly = false,
  isOmo = false,
  isRemovalProtected = false,
  isStateChangeProtected = false,
  defaultModelOptions = [],
  onSwitch = vi.fn(),
  onEdit = vi.fn(),
  onRemoveFromConfig = vi.fn(),
  onDelete = vi.fn(),
  onSetAsDefault = vi.fn(),
}: {
  isCurrent?: boolean;
  isInConfig?: boolean;
  isDefaultModel?: boolean;
  isReadOnly?: boolean;
  isOmo?: boolean;
  isRemovalProtected?: boolean;
  isStateChangeProtected?: boolean;
  defaultModelOptions?: { id: string; name?: string }[];
  onSwitch?: ReturnType<typeof vi.fn>;
  onEdit?: ReturnType<typeof vi.fn>;
  onRemoveFromConfig?: ReturnType<typeof vi.fn>;
  onDelete?: ReturnType<typeof vi.fn>;
  onSetAsDefault?: ReturnType<typeof vi.fn>;
}) {
  render(
    <ProviderActions
      appId="deepseek-harness"
      isCurrent={isCurrent}
      isInConfig={isInConfig}
      isDefaultModel={isDefaultModel}
      isReadOnly={isReadOnly}
      isOmo={isOmo}
      isRemovalProtected={isRemovalProtected}
      isStateChangeProtected={isStateChangeProtected}
      defaultModelOptions={defaultModelOptions}
      onSwitch={onSwitch}
      onRemoveFromConfig={onRemoveFromConfig}
      onSetAsDefault={onSetAsDefault}
      onEdit={onEdit}
      onDelete={onDelete}
    />,
  );
  return { onSwitch, onEdit, onRemoveFromConfig, onDelete, onSetAsDefault };
}

describe("ProviderActions DSH default model", () => {
  it("offers set-as-default for an in-config DSH provider", () => {
    renderDshActions({
      isInConfig: true,
      isDefaultModel: false,
      defaultModelOptions: [{ id: "k3" }],
    });

    expect(
      screen.getByRole("button", { name: "设为默认" }),
    ).toBeInTheDocument();
  });

  it("shows a model dropdown when multiple models exist", async () => {
    const user = userEvent.setup();
    const { onSetAsDefault } = renderDshActions({
      isInConfig: true,
      isDefaultModel: false,
      defaultModelOptions: [{ id: "k3" }, { id: "k3-turbo" }],
    });

    await user.click(screen.getByRole("button", { name: "设为默认" }));
    await user.click(screen.getByRole("menuitem", { name: "k3-turbo" }));

    expect(onSetAsDefault).toHaveBeenCalledWith("k3-turbo");
  });

  it("shows a disabled current-default state", () => {
    renderDshActions({
      isInConfig: true,
      isDefaultModel: true,
      defaultModelOptions: [{ id: "k3" }],
    });

    expect(screen.getByRole("button", { name: "当前默认" })).toBeDisabled();
  });

  it("offers remove-from-config for a non-current in-config DSH provider", async () => {
    const user = userEvent.setup();
    const { onRemoveFromConfig } = renderDshActions({
      isInConfig: true,
      isCurrent: false,
    });

    const removeButton = screen.getByRole("button", { name: "移除" });
    await user.click(removeButton);

    expect(onRemoveFromConfig).toHaveBeenCalledTimes(1);
  });

  it("hides remove-from-config for the current provider", () => {
    renderDshActions({
      isInConfig: true,
      isCurrent: true,
    });

    expect(
      screen.queryByRole("button", { name: "移除" }),
    ).not.toBeInTheDocument();
  });
});

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
