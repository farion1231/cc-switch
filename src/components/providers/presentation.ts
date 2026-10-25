import type { TFunction } from "i18next";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import type { AppMode } from "@/types/proxy";
import { isPiFamilyApp } from "@/config/appConfig";
import {
  isOfficialAccount,
  providerNeedsRouting,
  supportsFailover,
  supportsOfficialProxyTakeover,
} from "@/utils/providerCapabilities";

/**
 * 供应商卡片怎么画（v7）：当前那张的模式色、状态文字或按钮、徽标。由列表按应用和正在查看的
 * 模式算好交给卡片，卡片本身不再判断模式。
 */
export type CardTone = "direct" | "route" | "stack";

export type ChipTone =
  | "outline"
  | "direct"
  | "route"
  | "stack"
  | "success"
  | "warning"
  | "danger";

export interface CardChip {
  key: string;
  label: string;
  tone: ChipTone;
  title?: string;
}

export interface CardMenuOption {
  key: string;
  label: string;
  detail?: string;
  onSelect: () => void;
}

/** 「更多」菜单里跟当前模式有关的一项。 */
export interface CardMenuItem {
  key: string;
  label: string;
  /** 不能点的原因：照常列出来，原因写在名字下面 */
  disabledReason?: string;
  onSelect: () => void;
  /** 展开成子菜单选一项（OpenClaw / OpenCode 的「设为默认」选默认模型） */
  submenu?: { title: string; options: CardMenuOption[] };
}

export interface CardButton {
  key: string;
  label: string;
  onClick: () => void;
  /** 不能点的原因：按钮照常画出来，原因挂在按钮的说明卡上 */
  disabledReason?: string;
}

export interface CardPresentation {
  /** 当前那张：模式色边框 + 淡底；共存式应用的默认那家（OpenClaw / OpenCode / Kimi 默认、Hermes 当前）也用 direct */
  tone?: CardTone;
  /** 主操作位换成状态文字（使用中 / 路由中 / 当前默认 / 已添加…） */
  status?: { label: string; dot: CardTone | "muted" };
  buttons: CardButton[];
  /** 「更多」菜单最前面、跟当前模式有关的操作（聚合页、OpenClaw / OpenCode 的「设为默认」） */
  menuItems?: CardMenuItem[];
  chips: CardChip[];
  /** 整卡淡一些（不能用于当前模式的官方订阅、Hermes 托管） */
  dim?: boolean;
  /** 故障转移队列里的上移 / 下移（键盘也能排序） */
  move?: { onUp?: () => void; onDown?: () => void };
  /** 在队列里且路由服务在跑时显示健康状态 */
  showHealth?: boolean;
  /** 编辑 / 删除不可用的原因（当前那家不能删、Hermes 托管只能在 Web UI 改…） */
  editDisabledReason?: string;
  deleteDisabledReason?: string;
}

export interface ProviderSection {
  key: string;
  title?: string;
  help?: { title: string; body: string };
  emptyText?: string;
  items: { provider: Provider; presentation: CardPresentation }[];
}

/** 走路由时不能用的官方订阅（Codex 官方账号可以，靠客户端自己的登录）。 */
export function blockedFromRouting(app: AppId, provider: Provider): boolean {
  return (
    isOfficialAccount(app, provider) &&
    !supportsOfficialProxyTakeover(app, provider)
  );
}

// ─── 切换式应用（Claude Code / Codex / Gemini CLI / Grok Build）──────────────

/**
 * 故障转移队列里把一家上移 / 下移一格后，各家要写回的 sortIndex。队列顺序就是供应商的排序：
 * 按队列成员原来占着的位置重新分配。位置必须两两不同，否则换位后并列、顺序看起来没动——
 * 两家 sortIndex 相同，或新加的供应商没有 sortIndex、拿下标兜底时撞上别家的值，都会这样。
 * 移不动（不在队列 / 已到头）返回 null。
 */
export function queueMoveSortUpdates(
  items: { providerId: string; sortIndex?: number }[],
  providers: Record<string, Pick<Provider, "sortIndex">>,
  providerId: string,
  delta: -1 | 1,
): { id: string; sortIndex: number }[] | null {
  const index = items.findIndex((item) => item.providerId === providerId);
  const target = index + delta;
  if (index < 0 || target < 0 || target >= items.length) return null;
  const order = items.map((item) => item.providerId);
  order.splice(index, 1);
  order.splice(target, 0, providerId);
  const slots = items
    .map(
      (item, i) => item.sortIndex ?? providers[item.providerId]?.sortIndex ?? i,
    )
    .sort((a, b) => a - b);
  for (let i = 1; i < slots.length; i++) {
    if (slots[i] <= slots[i - 1]) slots[i] = slots[i - 1] + 1;
  }
  return order.map((id, i) => ({ id, sortIndex: slots[i] }));
}

export interface SwitchModeInput {
  app: AppId;
  t: TFunction;
  providers: Provider[];
  active: AppMode;
  view: AppMode;
  directId: string | null;
  routeId: string | null;
  failoverOn: boolean;
  /** 队列里的 id，按优先级 */
  queue: string[];
  /** 聚合名单（含默认那家）：id → 发布的模型数 */
  stackMembers: Map<string, number>;
  /** 需要路由的原因（悬停「需要路由」时的说明） */
  routingReason: (provider: Provider) => string;
  serviceRunning: boolean;
  actions: {
    switchDirect: (provider: Provider) => void;
    needsRouteDialog: (provider: Provider) => void;
    exitAndUse: (provider: Provider) => void;
    routeTo: (provider: Provider) => void;
    queueAdd: (provider: Provider) => void;
    queueRemove: (provider: Provider) => void;
    queueMove: (provider: Provider, delta: -1 | 1) => void;
    stackAdd: (provider: Provider) => void;
    stackRemove: (provider: Provider) => void;
    stackSetDefault: (provider: Provider) => void;
  };
}

export function buildSwitchSections(input: SwitchModeInput): ProviderSection[] {
  const { t, active, directId, routeId } = input;
  // 后端 is_referenced：直连指针指着的、路由 / 聚合模式下正在路由的，都删不掉。
  // 不管当前看的是哪个视图都要标出来，否则确认后后端拒绝、确认框还卡在原地
  // 路由 / 聚合时直连那家不算「正在用」，路由页卡上也不标，单独说明为什么删不掉
  const deleteBlockedReason = (id: string) => {
    if (active !== "direct" && id === routeId)
      return t("providerCard.reason.inUseCannotDelete");
    if (id === directId)
      return active === "direct"
        ? t("providerCard.reason.inUseCannotDelete")
        : t("providerCard.reason.directDefaultCannotDelete");
    return undefined;
  };
  return buildSwitchSectionsByView(input).map((section) => ({
    ...section,
    items: section.items.map((item) => {
      const reason = deleteBlockedReason(item.provider.id);
      return reason
        ? {
            ...item,
            presentation: {
              ...item.presentation,
              deleteDisabledReason: reason,
            },
          }
        : item;
    }),
  }));
}

function buildSwitchSectionsByView(input: SwitchModeInput): ProviderSection[] {
  const {
    app,
    t,
    providers,
    active,
    view,
    directId,
    routeId,
    failoverOn,
    queue,
    stackMembers,
    actions,
  } = input;
  // 早期绑定托管账号的 Codex 官方卡没有 category，按身份认
  const official = (p: Provider) => isOfficialAccount(app, p);

  const chip = {
    official: (): CardChip => ({
      key: "official",
      label: t("providerCard.chip.official"),
      tone: "outline",
    }),
    needsRoute: (p: Provider): CardChip => ({
      key: "needsRoute",
      label: t("providerCard.chip.needsRoute"),
      tone: "outline",
      title: input.routingReason(p),
    }),
    // 只在直连页上标：不用点开「回到直连」的确认框，在列表里就看得出会落到哪家。
    // 路由页上没有跟直连那家有关的操作，不标
    directWhenDirect: (): CardChip => ({
      key: "direct",
      label: t("providerCard.chip.directWhenDirect"),
      tone: "direct",
      title: t("provider.directProviderHint"),
    }),
  };
  const base = (p: Provider) => {
    const chips: CardChip[] = [];
    if (official(p)) chips.push(chip.official());
    if (providerNeedsRouting(app, p)) chips.push(chip.needsRoute(p));
    return chips;
  };
  const officialOnly = (p: Provider) => (official(p) ? [chip.official()] : []);
  const blocked = (p: Provider, label: string, reason: string) => ({
    provider: p,
    presentation: {
      chips: officialOnly(p),
      dim: true,
      buttons: [
        {
          key: "blocked",
          label,
          onClick: () => undefined,
          disabledReason: reason,
        },
      ],
    } satisfies CardPresentation,
  });

  if (view === "direct") {
    return [
      {
        key: "all",
        items: providers.map((p) => {
          const chips = base(p);
          if (active === "direct") {
            if (p.id === directId) {
              return {
                provider: p,
                presentation: {
                  tone: "direct",
                  status: {
                    label: t("providerCard.status.inUse"),
                    dot: "direct",
                  },
                  chips,
                  buttons: [],
                },
              };
            }
            return {
              provider: p,
              presentation: {
                chips,
                buttons: [
                  {
                    key: "switch",
                    label: t("providerCard.action.switch"),
                    onClick: () =>
                      providerNeedsRouting(app, p)
                        ? actions.needsRouteDialog(p)
                        : actions.switchDirect(p),
                  },
                ],
              },
            };
          }
          // 实际在路由 / 聚合，正在查看直连
          if (p.id === directId) chips.push(chip.directWhenDirect());
          return {
            provider: p,
            presentation: {
              chips,
              buttons: [
                {
                  key: "exitAndUse",
                  label: t("providerCard.action.backToDirectAndUse"),
                  onClick: () => actions.exitAndUse(p),
                  disabledReason: providerNeedsRouting(app, p)
                    ? t("providerCard.reason.needsRoute", {
                        reason: input.routingReason(p),
                      })
                    : undefined,
                },
              ],
            },
          };
        }),
      },
    ];
  }

  if (view === "route") {
    const noRoute = t("providerCard.reason.noRoute");
    if (active === "route" && !failoverOn) {
      return [
        {
          key: "all",
          items: providers.map((p) => {
            if (blockedFromRouting(app, p))
              return blocked(p, t("providerCard.action.routeHere"), noRoute);
            const chips = officialOnly(p);
            if (p.id === routeId) {
              return {
                provider: p,
                presentation: {
                  tone: "route",
                  status: {
                    label: t("providerCard.status.routing"),
                    dot: "route",
                  },
                  chips,
                  buttons: [],
                },
              };
            }
            return {
              provider: p,
              presentation: {
                chips,
                buttons: [
                  {
                    key: "routeHere",
                    label: t("providerCard.action.routeHere"),
                    onClick: () => actions.routeTo(p),
                  },
                ],
              },
            };
          }),
        },
      ];
    }

    if (active === "route") {
      // 故障转移开着：队列 + 不在队列。P 序号和上下移按完整队列算，`providers`
      // 可能被搜索过滤过、只决定画哪些卡；「路由中」是后端记下的那家（转移成功后
      // 会换），不一定是队首
      const byId = new Map(providers.map((p) => [p.id, p]));
      const routing =
        routeId !== null && queue.includes(routeId) ? routeId : queue[0];
      const queueItems = queue.flatMap((id, index) => {
        const p = byId.get(id);
        if (!p) return [];
        const current = id === routing;
        return [
          {
            provider: p,
            presentation: {
              tone: current ? "route" : undefined,
              status: current
                ? {
                    label: t("providerCard.status.routing"),
                    dot: "route" as const,
                  }
                : undefined,
              chips: [
                {
                  key: "priority",
                  label: `P${index + 1}`,
                  tone: "outline" as const,
                },
              ],
              showHealth: input.serviceRunning,
              buttons: [
                {
                  key: "queueRemove",
                  label: t("providerCard.action.removeFromQueue"),
                  onClick: () => actions.queueRemove(p),
                },
              ],
              move: {
                onUp: index > 0 ? () => actions.queueMove(p, -1) : undefined,
                onDown:
                  index < queue.length - 1
                    ? () => actions.queueMove(p, 1)
                    : undefined,
              },
            } satisfies CardPresentation,
          },
        ];
      });
      const rest = providers
        .filter((p) => !queue.includes(p.id))
        .map((p) => {
          if (blockedFromRouting(app, p))
            return blocked(p, t("providerCard.action.addToQueue"), noRoute);
          // Codex 官方账号卡能路由，但后端不让它进队列（靠客户端自己的登录，不能和别家轮换）
          if (!supportsFailover(app, p))
            return blocked(
              p,
              t("providerCard.action.addToQueue"),
              t("providerCard.reason.noFailover"),
            );
          return {
            provider: p,
            presentation: {
              chips: officialOnly(p),
              buttons: [
                {
                  key: "queueAdd",
                  label: t("providerCard.action.addToQueue"),
                  onClick: () => actions.queueAdd(p),
                },
              ],
            } satisfies CardPresentation,
          };
        });
      return [
        {
          key: "queue",
          title: t("providerCard.section.queue", { count: queueItems.length }),
          help: {
            title: t("providerCard.section.queueHelpTitle"),
            body: t("providerCard.section.queueHelp"),
          },
          emptyText: t("providerCard.section.queueEmpty"),
          items: queueItems,
        },
        {
          key: "rest",
          title: t("providerCard.section.notInQueue", { count: rest.length }),
          items: rest,
        },
      ];
    }

    // 实际在直连 / 聚合，正在查看路由：行上没有主操作，要生效只有通知条的「开始路由」，
    // 路由到哪家在确认框里选（预选上次那家，框里看得到，卡上不再标）。不能路由的官方订阅
    // 没有按钮可挂原因，原因挂在「官方」徽标上
    return [
      {
        key: "all",
        items: providers.map((p) => {
          if (blockedFromRouting(app, p))
            return {
              provider: p,
              presentation: {
                chips: [{ ...chip.official(), title: noRoute }],
                dim: true,
                buttons: [],
              } satisfies CardPresentation,
            };
          return {
            provider: p,
            presentation: {
              chips: officialOnly(p),
              buttons: [],
            } satisfies CardPresentation,
          };
        }),
      },
    ];
  }

  // view === "stack"
  const on = active === "stack";
  const defaultId = (() => {
    const candidate = on ? routeId : (routeId ?? directId);
    const p = providers.find((x) => x.id === candidate);
    return p && !blockedFromRouting(app, p) ? p.id : null;
  })();
  const defaultProvider = providers.find((p) => p.id === defaultId);
  // 行上的主操作只有名单的添加 / 移除；「设为默认」在「更多」菜单里：进了聚合当场生效，
  // 在直连时只记下选择（切换时的确认框里还能改）。正在路由时不能点：默认和路由目标是同一个
  // 指针，改了就是当场换路由
  const setDefaultItems = (p: Provider): CardMenuItem[] => [
    {
      key: "setDefault",
      label: t("providerCard.action.setDefault"),
      disabledReason:
        active === "route"
          ? t("providerCard.reason.defaultWhileRouting")
          : undefined,
      onSelect: () => actions.stackSetDefault(p),
    },
  ];
  const memberIds = providers
    .filter((p) => p.id !== defaultId && stackMembers.has(p.id))
    .map((p) => p.id);
  const modelsChip = (p: Provider): CardChip => {
    const count = stackMembers.get(p.id) ?? 0;
    return count > 0
      ? {
          key: "models",
          label: t("providerCard.chip.models", { count }),
          tone: "outline",
        }
      : {
          key: "models",
          label: t("providerCard.chip.noModels"),
          tone: "warning",
          title: t("provider.stackBadgeNoModels"),
        };
  };

  const defaultItems = defaultProvider
    ? [
        {
          provider: defaultProvider,
          presentation: {
            tone: on ? ("stack" as const) : undefined,
            status: on
              ? {
                  label: t("providerCard.status.currentDefault"),
                  dot: "stack" as const,
                }
              : {
                  label: t("providerCard.status.defaultAfterSwitch"),
                  dot: "muted" as const,
                },
            chips: [
              {
                key: "default",
                label: t("providerCard.chip.default"),
                tone: on ? ("outline" as const) : ("stack" as const),
              },
              ...officialOnly(defaultProvider),
            ],
            buttons: [],
          } satisfies CardPresentation,
        },
      ]
    : [];

  const memberItems = memberIds.map((id) => {
    const p = providers.find((x) => x.id === id)!;
    return {
      provider: p,
      presentation: {
        chips: [modelsChip(p)],
        menuItems: setDefaultItems(p),
        buttons: [
          {
            key: "remove",
            label: t("providerCard.action.remove"),
            onClick: () => actions.stackRemove(p),
          },
        ],
      } satisfies CardPresentation,
    };
  });

  const availItems = providers
    .filter((p) => p.id !== defaultId && !memberIds.includes(p.id))
    .map((p) => {
      if (blockedFromRouting(app, p)) {
        return blocked(
          p,
          t("providerCard.action.add"),
          t("providerCard.reason.noStack"),
        );
      }
      if (official(p)) {
        return {
          provider: p,
          presentation: {
            chips: [chip.official()],
            menuItems: setDefaultItems(p),
            buttons: [
              {
                key: "add",
                label: t("providerCard.action.add"),
                onClick: () => undefined,
                disabledReason: t("providerCard.reason.officialStack"),
              },
            ],
          } satisfies CardPresentation,
        };
      }
      return {
        provider: p,
        presentation: {
          chips: providerNeedsRouting(app, p) ? [chip.needsRoute(p)] : [],
          // 还没添加的也能直接设为默认：后端会把默认那家一起加进名单
          menuItems: setDefaultItems(p),
          buttons: [
            {
              key: "add",
              label: t("providerCard.action.add"),
              onClick: () => actions.stackAdd(p),
            },
          ],
        } satisfies CardPresentation,
      };
    });

  return [
    {
      key: "default",
      title: on
        ? t("providerCard.section.stackDefault")
        : t("providerCard.section.stackDefaultAfterSwitch"),
      help: {
        title: t("providerCard.section.stackDefault"),
        body: t(
          app === "claude-desktop"
            ? "providerCard.section.stackDefaultHelpDesktop"
            : "providerCard.section.stackDefaultHelp",
        ),
      },
      emptyText: t("providerCard.section.stackNoDefault"),
      items: defaultItems,
    },
    {
      key: "members",
      title: t("providerCard.section.added", { count: memberItems.length }),
      emptyText: t("providerCard.section.stackEmpty"),
      items: memberItems,
    },
    {
      key: "available",
      title: t("providerCard.section.available", { count: availItems.length }),
      items: availItems,
    },
  ];
}

// ─── 启动器式应用（Antigravity）──────────────────────────────────────────────

export interface LaunchInput {
  app: AppId;
  t: TFunction;
  providers: Provider[];
  openTerminal: (provider: Provider) => void;
}

/**
 * Antigravity：第三方 Key 只能经环境变量交给 CC Switch 打开的那个 agy，管不到用户自己敲的
 * `agy`，所以没有「启用 / 使用中」，每张卡的主操作就是「打开终端」，哪张都能删。
 */
export function buildLaunchSections(input: LaunchInput): ProviderSection[] {
  const { app, t, providers, openTerminal } = input;
  return [
    {
      key: "all",
      items: providers.map((p) => ({
        provider: p,
        presentation: {
          chips: isOfficialAccount(app, p)
            ? [
                {
                  key: "official",
                  label: t("providerCard.chip.official"),
                  tone: "outline",
                },
              ]
            : [],
          buttons: [
            {
              key: "openTerminal",
              label: t("provider.openTerminalWithProvider"),
              onClick: () => openTerminal(p),
            },
          ],
        },
      })),
    },
  ];
}

// ─── 共存式应用（OpenCode / OpenClaw / Hermes / Pi / MiniMax Code / Kimi）─────

export interface AdditiveInput {
  app: AppId;
  t: TFunction;
  providers: Provider[];
  isInConfig: (provider: Provider) => boolean;
  /** OpenCode：当前启用的 OMO / OMO Slim */
  currentOmoId?: string | null;
  currentOmoSlimId?: string | null;
  /** OpenClaw / OpenCode：默认模型属于哪家、它的模型名 */
  defaultModel?: { providerId: string; model: string } | null;
  /** OpenClaw / OpenCode：每家可选的模型 */
  defaultModelOptions?: (provider: Provider) => { id: string; name?: string }[];
  /** Hermes：model.provider 指向的那家 */
  hermesCurrentId?: string | null;
  isHermesManaged?: (provider: Provider) => boolean;
  /** Pi / Oh My Pi：读不到当前配置时一律不能改 */
  piStateUnavailable?: boolean;
  /** Kimi：原生 default_model 别名所属的供应商。 */
  kimiCurrentId?: string | null;
  actions: {
    add: (provider: Provider) => void;
    remove: (provider: Provider) => void;
    disableOmo: (provider: Provider) => void;
    setDefault: (provider: Provider, modelId?: string) => void;
  };
}

export function buildAdditiveSections(input: AdditiveInput): ProviderSection[] {
  const { app, t, providers, actions } = input;
  const added: ProviderSection["items"] = [];
  const available: ProviderSection["items"] = [];
  const piBlocked =
    isPiFamilyApp(app) && input.piStateUnavailable
      ? t(`${app}.current.stateUnavailableHint`)
      : undefined;

  for (const p of providers) {
    const chips: CardChip[] = [];
    if (app === "kimi") {
      const official = isOfficialAccount(app, p);
      const live = input.isInConfig(p);
      // 官方卡可以编辑模型、可以移出 config.toml（登录留着，能再添加）；仍在 Kimi 里的
      // 官方卡不能直接删，删号在授权中心（连登录一起删）
      const deleteProtectedReason =
        official && live ? t("kimi.oauth.managedHint") : undefined;
      if (official) {
        chips.push({
          key: "official",
          label: t("providerCard.chip.official"),
          tone: "outline",
        });
      }
      const current = live && p.id === input.kimiCurrentId;
      // 「添加」只写进 config.toml（模型出现在 Kimi 的 /model 列表），「移除」从 config.toml
      // 拿掉、卡片留着；改 default_model 的「设为默认」和 OpenCode 一样收在「更多」菜单里
      const buttons: CardButton[] = [];
      const menuItems: CardMenuItem[] = [];
      if (!live) {
        buttons.push({
          key: "add",
          label: t("providerCard.action.add"),
          onClick: () => actions.add(p),
        });
      } else {
        if (!current) {
          menuItems.push({
            key: "setDefault",
            label: t("providerCard.action.setDefault"),
            onSelect: () => actions.setDefault(p),
          });
        }
        buttons.push({
          key: "remove",
          label: t("providerCard.action.remove"),
          disabledReason: current
            ? t("providerCard.reason.currentCannotRemove")
            : undefined,
          onClick: () => actions.remove(p),
        });
      }
      (live ? added : available).push({
        provider: p,
        presentation: {
          tone: current ? "direct" : undefined,
          status: current
            ? { label: t("providerCard.status.default"), dot: "direct" }
            : undefined,
          chips,
          buttons,
          menuItems: menuItems.length ? menuItems : undefined,
          deleteDisabledReason:
            deleteProtectedReason ??
            (current ? t("providerCard.reason.inUseCannotDelete") : undefined),
        },
      });
      continue;
    }
    const isOmo = p.category === "omo";
    const isOmoSlim = p.category === "omo-slim";
    if (isOmo) chips.push({ key: "omo", label: "OMO", tone: "outline" });
    if (isOmoSlim) chips.push({ key: "slim", label: "Slim", tone: "outline" });

    // OMO / Slim：插件配置，同一时间只启用一个
    if (isOmo || isOmoSlim) {
      const enabled = isOmo
        ? p.id === input.currentOmoId
        : p.id === input.currentOmoSlimId;
      if (enabled) {
        // 分组标题已经写了「已添加」，按钮是「停用」，卡上不再重复写状态
        added.push({
          provider: p,
          presentation: {
            chips,
            buttons: [
              {
                key: "disable",
                label: t("providerCard.action.disable"),
                onClick: () => actions.disableOmo(p),
              },
            ],
          },
        });
      } else {
        available.push({
          provider: p,
          presentation: {
            chips,
            buttons: [
              {
                key: "enable",
                label: t("providerCard.action.enable"),
                onClick: () => actions.add(p),
              },
            ],
          },
        });
      }
      continue;
    }

    const managed = app === "hermes" && (input.isHermesManaged?.(p) ?? false);
    if (managed) {
      chips.push({
        key: "managed",
        label: t("providerCard.chip.hermesManaged"),
        tone: "outline",
        title: t("provider.managedByHermesHint"),
      });
    }
    const readOnlyReason = managed
      ? t("provider.managedByHermesHint")
      : undefined;

    if (!input.isInConfig(p)) {
      available.push({
        provider: p,
        presentation: {
          chips,
          dim: managed,
          editDisabledReason: readOnlyReason,
          deleteDisabledReason: readOnlyReason ?? piBlocked,
          buttons: [
            {
              key: "add",
              label: isPiFamilyApp(app)
                ? t("providerCard.action.enable")
                : t("providerCard.action.add"),
              onClick: () => actions.add(p),
              disabledReason: piBlocked,
            },
          ],
        },
      });
      continue;
    }

    // 已添加：白底、不写「● 已添加」（分组标题和「移除」已经说清楚）；蓝底只给默认那一家
    const buttons: CardButton[] = [];
    const menuItems: CardMenuItem[] = [];
    let status: CardPresentation["status"];
    let isDefault = false;
    let removeReason: string | undefined = piBlocked;

    // OpenCode 的顶层 model 一直允许移走所指的那家，这里不改；OpenClaw 不能
    if (app === "openclaw" || app === "opencode") {
      isDefault = input.defaultModel?.providerId === p.id;
      if (isDefault) {
        status = { label: t("providerCard.status.default"), dot: "direct" };
        chips.push({
          key: "defaultModel",
          label: t("providerCard.chip.defaultModel", {
            model: input.defaultModel?.model ?? "",
          }),
          tone: "outline",
        });
        if (app === "openclaw") {
          removeReason = t("providerCard.reason.defaultCannotRemove");
        }
      } else {
        const models = input.defaultModelOptions?.(p) ?? [];
        // 「设为默认」放「更多」菜单，不占卡片上的主操作位；多个模型时展开子菜单选。
        // OpenCode 内置供应商可以不列模型（运行时才有），给不出选项就不显示
        if (app === "openclaw" || models.length > 0) {
          menuItems.push({
            key: "setDefault",
            label: t("providerCard.action.setDefault"),
            onSelect: () => actions.setDefault(p, models[0]?.id),
            submenu:
              models.length > 1
                ? {
                    title: t("openclaw.selectDefaultModel"),
                    options: models.map((model) => ({
                      key: model.id,
                      label: model.name?.trim() || model.id,
                      detail:
                        model.name?.trim() && model.name.trim() !== model.id
                          ? model.id
                          : undefined,
                      onSelect: () => actions.setDefault(p, model.id),
                    })),
                  }
                : undefined,
          });
        }
      }
    }

    if (app === "hermes") {
      if (p.id === input.hermesCurrentId) {
        isDefault = true;
        status = { label: t("providerCard.status.current"), dot: "direct" };
        removeReason = t("providerCard.reason.currentCannotRemove");
      } else {
        buttons.push({
          key: "use",
          label: t("providerCard.action.enable"),
          onClick: () => actions.setDefault(p),
          disabledReason: readOnlyReason,
        });
      }
    }

    buttons.push({
      key: "remove",
      label: t("providerCard.action.remove"),
      onClick: () => actions.remove(p),
      disabledReason: removeReason ?? readOnlyReason,
    });

    added.push({
      provider: p,
      presentation: {
        tone: isDefault ? "direct" : undefined,
        status,
        chips,
        dim: managed,
        editDisabledReason: readOnlyReason,
        deleteDisabledReason: readOnlyReason ?? piBlocked,
        buttons,
        menuItems: menuItems.length ? menuItems : undefined,
      },
    });
  }

  return [
    {
      key: "added",
      title: t("providerCard.section.added", { count: added.length }),
      help: {
        title: t("providerCard.section.addedHelpTitle"),
        body: t("providerCard.section.addedHelp", {
          app: t(`apps.${app}`),
        }),
      },
      emptyText: t("providerCard.section.addedEmpty"),
      items: added,
    },
    {
      key: "available",
      title: t("providerCard.section.available", { count: available.length }),
      items: available,
    },
  ];
}
