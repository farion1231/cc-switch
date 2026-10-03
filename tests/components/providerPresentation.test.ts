import { describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import type { Provider } from "@/types";
import type { AppMode } from "@/types/proxy";
import {
  buildAdditiveSections,
  buildDesktopSections,
  buildSwitchSections,
  type ProviderSection,
  type SwitchModeInput,
} from "@/components/providers/presentation";

const t = ((key: string, options?: Record<string, unknown>) =>
  options && "count" in options
    ? `${key}:${String(options.count)}`
    : key) as unknown as TFunction;

const provider = (id: string, overrides: Partial<Provider> = {}): Provider => ({
  id,
  name: id,
  settingsConfig: {},
  ...overrides,
});

const official = provider("official", { category: "official" });
const relay = provider("relay");
const backup = provider("backup");
/** Claude 下走 OpenAI 格式：需要路由 */
const converted = provider("converted", { meta: { apiFormat: "openai_chat" } });

function actions() {
  return {
    switchDirect: vi.fn(),
    needsRouteDialog: vi.fn(),
    exitAndUse: vi.fn(),
    routeTo: vi.fn(),
    queueAdd: vi.fn(),
    queueRemove: vi.fn(),
    queueMove: vi.fn(),
    stackAdd: vi.fn(),
    stackRemove: vi.fn(),
    stackSetDefault: vi.fn(),
  };
}

function build(
  overrides: Partial<SwitchModeInput> & { active: AppMode; view: AppMode },
) {
  const input: SwitchModeInput = {
    app: "claude",
    t,
    providers: [official, relay, backup, converted],
    directId: "relay",
    routeId: null,
    failoverOn: false,
    queue: [],
    stackMembers: new Map(),
    routingReason: () => "reason",
    serviceRunning: true,
    actions: actions(),
    ...overrides,
  };
  return { sections: buildSwitchSections(input), input };
}

const item = (sections: ProviderSection[], id: string) => {
  for (const section of sections) {
    const found = section.items.find((entry) => entry.provider.id === id);
    if (found) return { section: section.key, ...found.presentation };
  }
  throw new Error(`no card for ${id}`);
};

const button = (sections: ProviderSection[], id: string, key: string) => {
  const found = item(sections, id).buttons.find((b) => b.key === key);
  if (!found) throw new Error(`no ${key} button on ${id}`);
  return found;
};

/** 「更多」菜单里的「设为默认」 */
const setDefaultItem = (sections: ProviderSection[], id: string) => {
  const found = item(sections, id).menuItems?.find(
    (m) => m.key === "setDefault",
  );
  if (!found) throw new Error(`no setDefault menu item on ${id}`);
  return found;
};

describe("buildSwitchSections — direct", () => {
  it("marks the direct provider in use and asks before switching to one that needs routing", () => {
    const { sections, input } = build({ active: "direct", view: "direct" });

    expect(item(sections, "relay")).toMatchObject({
      tone: "direct",
      status: { label: "providerCard.status.inUse", dot: "direct" },
      buttons: [],
    });
    button(sections, "backup", "switch").onClick();
    expect(input.actions.switchDirect).toHaveBeenCalledWith(backup);

    button(sections, "converted", "switch").onClick();
    expect(input.actions.needsRouteDialog).toHaveBeenCalledWith(converted);
    expect(item(sections, "converted").chips.map((c) => c.key)).toContain(
      "needsRoute",
    );
  });

  it("offers back-to-direct while viewing direct from routing, except for providers that need routing", () => {
    const { sections, input } = build({
      active: "route",
      view: "direct",
      routeId: "backup",
    });

    expect(item(sections, "relay").chips.map((c) => c.key)).toContain("direct");
    button(sections, "backup", "exitAndUse").onClick();
    expect(input.actions.exitAndUse).toHaveBeenCalledWith(backup);
    expect(
      button(sections, "converted", "exitAndUse").disabledReason,
    ).toBeTruthy();
  });
});

describe("buildSwitchSections — delete guard", () => {
  // 后端 is_referenced：直连指针、路由 / 聚合模式下的当前路由都删不掉，哪个视图都要标出来
  it("disables delete on the direct provider in every view", () => {
    for (const view of ["direct", "route", "stack"] as AppMode[]) {
      const { sections } = build({ active: "direct", view });
      expect(item(sections, "relay").deleteDisabledReason).toBe(
        "providerCard.reason.inUseCannotDelete",
      );
      expect(item(sections, "backup").deleteDisabledReason).toBeUndefined();
    }
  });

  it("also disables delete on the provider being routed to while routing", () => {
    const { sections } = build({
      active: "route",
      view: "direct",
      routeId: "backup",
    });
    expect(item(sections, "relay").deleteDisabledReason).toBeTruthy();
    expect(item(sections, "backup").deleteDisabledReason).toBeTruthy();
    expect(item(sections, "converted").deleteDisabledReason).toBeUndefined();

    // 直连时 routeId 只是上次的路由，不算在用
    const direct = build({
      active: "direct",
      view: "route",
      routeId: "backup",
    });
    expect(
      item(direct.sections, "backup").deleteDisabledReason,
    ).toBeUndefined();
  });
});

describe("buildSwitchSections — route", () => {
  it("routes to a provider and keeps official subscriptions out", () => {
    const { sections, input } = build({
      active: "route",
      view: "route",
      routeId: "backup",
    });

    expect(item(sections, "backup")).toMatchObject({
      tone: "route",
      status: { label: "providerCard.status.routing" },
    });
    button(sections, "relay", "routeHere").onClick();
    expect(input.actions.routeTo).toHaveBeenCalledWith(relay);
    // 路由页上没有跟直连那家有关的操作，不标「直连时使用」
    expect(item(sections, "relay").chips).toEqual([]);
    expect(item(sections, "official").dim).toBe(true);
    expect(button(sections, "official", "blocked").disabledReason).toBe(
      "providerCard.reason.noRoute",
    );
  });

  it("splits the failover queue from the rest and only moves within the queue", () => {
    const { sections, input } = build({
      active: "route",
      view: "route",
      routeId: "relay",
      failoverOn: true,
      queue: ["relay", "backup"],
    });

    expect(sections.map((s) => [s.key, s.items.length])).toEqual([
      ["queue", 2],
      ["rest", 2],
    ]);
    expect(item(sections, "relay")).toMatchObject({
      tone: "route",
      showHealth: true,
      chips: [{ key: "priority", label: "P1" }],
    });
    expect(item(sections, "relay").move?.onUp).toBeUndefined();
    item(sections, "relay").move?.onDown?.();
    expect(input.actions.queueMove).toHaveBeenCalledWith(relay, 1);
    expect(item(sections, "backup").move?.onDown).toBeUndefined();

    button(sections, "converted", "queueAdd").onClick();
    expect(input.actions.queueAdd).toHaveBeenCalledWith(converted);
    button(sections, "backup", "queueRemove").onClick();
    expect(input.actions.queueRemove).toHaveBeenCalledWith(backup);

    // 直连那家不在队列里时也不标「直连时使用」
    const outside = build({
      active: "route",
      view: "route",
      routeId: "backup",
      failoverOn: true,
      queue: ["backup"],
    });
    expect(item(outside.sections, "relay")).toMatchObject({
      section: "rest",
      chips: [],
    });
  });

  it("numbers the queue by its full order and marks the recorded route, not the first visible card", () => {
    // 搜索把 P1 过滤掉了：剩下的卡序号、上下移仍按完整队列算
    const { sections, input } = build({
      active: "route",
      view: "route",
      routeId: "backup",
      failoverOn: true,
      queue: ["relay", "backup", "converted"],
      providers: [backup, converted],
    });

    expect(item(sections, "backup").chips).toEqual([
      { key: "priority", label: "P2", tone: "outline" },
    ]);
    expect(item(sections, "converted").chips[0].label).toBe("P3");
    item(sections, "backup").move?.onUp?.();
    expect(input.actions.queueMove).toHaveBeenCalledWith(backup, -1);
    expect(item(sections, "converted").move?.onDown).toBeUndefined();

    // 转移成功后后端记下的路由是 backup：「路由中」在它身上，不在队首
    expect(item(sections, "backup")).toMatchObject({
      tone: "route",
      status: { label: "providerCard.status.routing" },
    });
    expect(item(sections, "converted").status).toBeUndefined();

    // 记下的路由已不在队列里：退回队首
    const fallback = build({
      active: "route",
      view: "route",
      routeId: "official",
      failoverOn: true,
      queue: ["relay", "backup"],
    });
    expect(item(fallback.sections, "relay").tone).toBe("route");
    expect(item(fallback.sections, "backup").tone).toBeUndefined();
  });

  it("keeps Codex official accounts out of the failover queue with a reason", () => {
    // 后端 require_failover_provider 会拒绝；卡上要先说清楚，而不是点了没反应
    const managed = provider("managed", {
      settingsConfig: { auth: {}, config: "" },
      meta: {
        authBinding: {
          source: "managed_account",
          authProvider: "codex_oauth",
          accountId: "acct",
        },
      },
    } as Partial<Provider>);
    const { sections, input } = build({
      app: "codex",
      active: "route",
      view: "route",
      routeId: "relay",
      failoverOn: true,
      queue: ["relay"],
      providers: [relay, backup, managed],
    });

    expect(item(sections, "managed").dim).toBe(true);
    expect(button(sections, "managed", "blocked").disabledReason).toBe(
      "providerCard.reason.noFailover",
    );
    button(sections, "backup", "queueAdd").onClick();
    expect(input.actions.queueAdd).toHaveBeenCalledWith(backup);
  });

  it("offers no row action while previewing route from direct", () => {
    const { sections } = build({
      active: "direct",
      view: "route",
      routeId: "backup",
    });

    // 进入路由只有通知条一个入口（目标在确认框里选、预选上次那家），行上没有主操作，
    // 也不标上次路由的那家和直连那家
    for (const id of ["relay", "backup", "converted"]) {
      expect(item(sections, id).buttons).toEqual([]);
      expect(item(sections, id).chips).toEqual([]);
      expect(item(sections, id).menuItems).toBeUndefined();
    }
    // 不能路由的官方订阅没有按钮可挂原因，原因在「官方」徽标上
    expect(item(sections, "official")).toMatchObject({
      dim: true,
      buttons: [],
      chips: [{ key: "official", title: "providerCard.reason.noRoute" }],
    });
  });
});

describe("buildSwitchSections — stack", () => {
  it("shows the default, members and available providers", () => {
    const { sections, input } = build({
      active: "stack",
      view: "stack",
      routeId: "relay",
      stackMembers: new Map([
        ["relay", 2],
        ["backup", 0],
      ]),
    });

    expect(sections.map((s) => [s.key, s.items.length])).toEqual([
      ["default", 1],
      ["members", 1],
      ["available", 2],
    ]);
    expect(item(sections, "relay")).toMatchObject({
      section: "default",
      tone: "stack",
      status: { label: "providerCard.status.currentDefault" },
    });
    // 发布了 0 个模型的成员要提醒
    expect(item(sections, "backup").chips[0]).toMatchObject({
      key: "models",
      tone: "warning",
    });
    // 行上只有名单的移除 / 添加；「设为默认」在「更多」菜单里
    expect(item(sections, "backup").buttons.map((b) => b.key)).toEqual([
      "remove",
    ]);
    button(sections, "backup", "remove").onClick();
    expect(input.actions.stackRemove).toHaveBeenCalledWith(backup);
    expect(setDefaultItem(sections, "backup")).toMatchObject({
      label: "providerCard.action.setDefault",
      disabledReason: undefined,
    });
    setDefaultItem(sections, "backup").onSelect();
    expect(input.actions.stackSetDefault).toHaveBeenCalledWith(backup);

    button(sections, "converted", "add").onClick();
    expect(input.actions.stackAdd).toHaveBeenCalledWith(converted);
    // 还没添加的也能直接设为默认
    setDefaultItem(sections, "converted").onSelect();
    expect(input.actions.stackSetDefault).toHaveBeenCalledWith(converted);
    // 默认那家自己没有这一项
    expect(item(sections, "relay").menuItems).toBeUndefined();
    // Claude 官方订阅不能进聚合，也做不了默认
    expect(button(sections, "official", "blocked").disabledReason).toBe(
      "providerCard.reason.noStack",
    );
    expect(item(sections, "official").menuItems).toBeUndefined();
  });

  it("previews the stack outside Stack mode: rows only edit the list", () => {
    const { sections, input } = build({
      active: "direct",
      view: "stack",
      routeId: null,
      stackMembers: new Map([["backup", 1]]),
    });

    // 没有路由目标时，默认那家落在直连那家
    expect(item(sections, "relay")).toMatchObject({
      section: "default",
      tone: undefined,
      status: { label: "providerCard.status.defaultAfterSwitch" },
    });
    // 行上没有会切模式的按钮，只有名单的添加 / 移除
    expect(item(sections, "backup").buttons.map((b) => b.key)).toEqual([
      "remove",
    ]);
    expect(item(sections, "converted").buttons.map((b) => b.key)).toEqual([
      "add",
    ]);
    button(sections, "converted", "add").onClick();
    expect(input.actions.stackAdd).toHaveBeenCalledWith(converted);
    button(sections, "backup", "remove").onClick();
    expect(input.actions.stackRemove).toHaveBeenCalledWith(backup);
    expect(input.actions.stackSetDefault).not.toHaveBeenCalled();
    // 「设为默认」在「更多」菜单里：直连时能点（只记下选择）
    expect(setDefaultItem(sections, "backup").disabledReason).toBeUndefined();
    setDefaultItem(sections, "converted").onSelect();
    expect(input.actions.stackSetDefault).toHaveBeenCalledWith(converted);
  });

  it("does not let the Stack default change from the preview while routing", () => {
    // 默认和路由目标是同一个指针：路由中改它就是当场换路由
    const { sections } = build({
      active: "route",
      view: "stack",
      routeId: "relay",
      stackMembers: new Map([["backup", 1]]),
    });

    expect(item(sections, "relay").section).toBe("default");
    for (const id of ["backup", "converted"]) {
      expect(setDefaultItem(sections, id).disabledReason).toBe(
        "providerCard.reason.defaultWhileRouting",
      );
    }
  });

  it("shows a Codex official account outside Stack mode with only the reason it cannot be added", () => {
    const account = provider("account", {
      category: "official",
      settingsConfig: { auth: {}, config: "" },
    });
    const { sections } = build({
      app: "codex",
      active: "direct",
      view: "stack",
      providers: [relay, account],
    });

    expect(item(sections, "account").section).toBe("available");
    expect(item(sections, "account").buttons.map((b) => b.key)).toEqual([
      "add",
    ]);
    expect(button(sections, "account", "add").disabledReason).toBe(
      "providerCard.reason.officialStack",
    );
    // 它能做默认
    expect(setDefaultItem(sections, "account").disabledReason).toBeUndefined();
  });

  it("never lets ChatGPT accounts be added in Codex Stack mode, even legacy cards without a category", () => {
    const managed = provider("managed", {
      settingsConfig: { auth: {}, config: "" },
      meta: {
        authBinding: {
          source: "managed_account",
          authProvider: "codex_oauth",
          accountId: "acct",
        },
      },
    } as Partial<Provider>);
    const { sections, input } = build({
      app: "codex",
      active: "stack",
      view: "stack",
      providers: [relay, backup, managed],
      routeId: "relay",
      stackMembers: new Map([["relay", 1]]),
    });

    expect(item(sections, "managed").chips.map((c) => c.key)).toContain(
      "official",
    );
    expect(button(sections, "managed", "add").disabledReason).toBe(
      "providerCard.reason.officialStack",
    );
    setDefaultItem(sections, "managed").onSelect();
    expect(input.actions.stackSetDefault).toHaveBeenCalledWith(managed);
  });
});

describe("buildDesktopSections", () => {
  it("colors the current card by whether it uses model mapping", () => {
    const mapped = provider("mapped", { meta: { claudeDesktopMode: "proxy" } });
    const onSwitch = vi.fn();
    const sections = buildDesktopSections({
      t,
      providers: [mapped, relay],
      currentId: "mapped",
      onSwitch,
    });

    expect(item(sections, "mapped")).toMatchObject({
      tone: "route",
      status: { label: "providerCard.status.inUse", dot: "route" },
      deleteDisabledReason: "providerCard.reason.inUseCannotDelete",
    });
    expect(item(sections, "relay").chips.map((c) => c.key)).toEqual(["direct"]);
    expect(item(sections, "relay").deleteDisabledReason).toBeUndefined();
    button(sections, "relay", "switch").onClick();
    expect(onSwitch).toHaveBeenCalledWith(relay);
  });
});

describe("buildAdditiveSections", () => {
  const additiveActions = () => ({
    add: vi.fn(),
    remove: vi.fn(),
    disableOmo: vi.fn(),
    setDefault: vi.fn(),
  });

  it("splits added and available providers and keeps OMO to one at a time", () => {
    const omo = provider("omo", { category: "omo" });
    const otherOmo = provider("omo-2", { category: "omo" });
    const actions = additiveActions();
    const sections = buildAdditiveSections({
      app: "opencode",
      t,
      providers: [relay, backup, omo, otherOmo],
      isInConfig: (p) => p.id === "relay",
      currentOmoId: "omo",
      actions,
    });

    expect(sections.map((s) => [s.key, s.items.length])).toEqual([
      ["added", 2],
      ["available", 2],
    ]);
    button(sections, "relay", "remove").onClick();
    expect(actions.remove).toHaveBeenCalledWith(relay);
    // 已添加的卡白底、不再写「● 已添加 / 已启用」：灰底只给默认那一家
    for (const id of ["relay", "omo"]) {
      expect(item(sections, id).tone).toBeUndefined();
      expect(item(sections, id).status).toBeUndefined();
    }
    button(sections, "backup", "add").onClick();
    expect(actions.add).toHaveBeenCalledWith(backup);
    button(sections, "omo", "disable").onClick();
    expect(actions.disableOmo).toHaveBeenCalledWith(omo);
    expect(item(sections, "omo-2").buttons.map((b) => b.key)).toEqual([
      "enable",
    ]);
  });

  it("lets OpenClaw pick the default model and protects the current default", () => {
    const multi = provider("multi", {
      settingsConfig: {
        models: [{ id: "m-1", name: "Model 1" }, { id: "m-2" }],
      },
    });
    const actions = additiveActions();
    const sections = buildAdditiveSections({
      app: "openclaw",
      t,
      providers: [relay, multi],
      isInConfig: () => true,
      openclawDefault: { providerId: "relay", model: "gpt" },
      openclawModels: (p) =>
        (p.settingsConfig as { models?: { id: string; name?: string }[] })
          .models ?? [],
      actions,
    });

    expect(item(sections, "relay")).toMatchObject({
      tone: "neutral",
      status: { label: "providerCard.status.default" },
    });
    expect(item(sections, "multi").tone).toBeUndefined();
    expect(item(sections, "multi").status).toBeUndefined();
    expect(button(sections, "relay", "remove").disabledReason).toBe(
      "providerCard.reason.defaultCannotRemove",
    );
    const setDefault = button(sections, "multi", "setDefault");
    expect(setDefault.menu?.options.map((o) => [o.label, o.detail])).toEqual([
      ["Model 1", "m-1"],
      ["m-2", undefined],
    ]);
    setDefault.menu?.options[1].onSelect();
    expect(actions.setDefault).toHaveBeenCalledWith(multi, "m-2");
  });

  it("keeps Hermes-managed providers read-only", () => {
    const managed = provider("managed");
    const sections = buildAdditiveSections({
      app: "hermes",
      t,
      providers: [relay, managed],
      isInConfig: () => true,
      hermesCurrentId: "relay",
      isHermesManaged: (p) => p.id === "managed",
      actions: additiveActions(),
    });

    expect(button(sections, "relay", "remove").disabledReason).toBe(
      "providerCard.reason.currentCannotRemove",
    );
    expect(item(sections, "relay").tone).toBe("neutral");
    expect(item(sections, "managed").tone).toBeUndefined();
    expect(item(sections, "managed")).toMatchObject({
      dim: true,
      editDisabledReason: "provider.managedByHermesHint",
      deleteDisabledReason: "provider.managedByHermesHint",
    });
    expect(button(sections, "managed", "use").disabledReason).toBe(
      "provider.managedByHermesHint",
    );
  });

  it("blocks every Pi change while Pi's current state is unavailable", () => {
    const sections = buildAdditiveSections({
      app: "pi",
      t,
      providers: [relay],
      isInConfig: () => false,
      piStateUnavailable: true,
      actions: additiveActions(),
    });

    expect(button(sections, "relay", "add")).toMatchObject({
      label: "providerCard.action.enable",
      disabledReason: "pi.current.stateUnavailableHint",
    });
    expect(item(sections, "relay").deleteDisabledReason).toBe(
      "pi.current.stateUnavailableHint",
    );
  });
});
