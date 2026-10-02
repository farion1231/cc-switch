import type { TFunction } from "i18next";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import type { AppMode } from "@/types/proxy";
import {
  isOfficialAccount,
  providerNeedsRouting,
  supportsOfficialProxyTakeover,
} from "@/utils/providerCapabilities";

/**
 * 供应商卡片怎么画（v7）：当前那张的模式色、状态文字或按钮、徽标。由列表按应用和正在查看的
 * 模式算好交给卡片，卡片本身不再判断模式。
 */
export type CardTone = "direct" | "route" | "stack" | "neutral";

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

export interface CardButton {
  key: string;
  label: string;
  onClick: () => void;
  /** 不能点的原因：按钮照常画出来，原因挂在按钮的说明卡上 */
  disabledReason?: string;
  /** 点开选一项（OpenClaw 的「设为默认 ▾」选默认模型） */
  menu?: { title: string; options: CardMenuOption[] };
}

export interface CardPresentation {
  /** 当前那张：模式色边框 + 淡底；neutral（灰底）只给共存式应用的默认那家（OpenClaw 默认、Hermes 当前） */
  tone?: CardTone;
  /** 主操作位换成状态文字（使用中 / 路由中 / 当前默认 / 已添加…） */
  status?: { label: string; dot: CardTone | "muted" };
  buttons: CardButton[];
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
  /** 叠加名单（含默认那家）：id → 发布的模型数 */
  stackMembers: Map<string, number>;
  /** 需要路由的原因（悬停「需要路由」时的说明） */
  routingReason: (provider: Provider) => string;
  serviceRunning: boolean;
  actions: {
    switchDirect: (provider: Provider) => void;
    needsRouteDialog: (provider: Provider) => void;
    exitAndUse: (provider: Provider) => void;
    routeTo: (provider: Provider) => void;
    startRouteFrom: (provider: Provider) => void;
    /** 没进叠加时，以这家为默认打开进叠加的确认框（同路由预览的「从这家开始路由」） */
    startStackFrom: (provider: Provider) => void;
    queueAdd: (provider: Provider) => void;
    queueRemove: (provider: Provider) => void;
    queueMove: (provider: Provider, delta: -1 | 1) => void;
    stackAdd: (provider: Provider) => void;
    stackRemove: (provider: Provider) => void;
    stackSetDefault: (provider: Provider) => void;
  };
}

export function buildSwitchSections(input: SwitchModeInput): ProviderSection[] {
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
    directWhenDirect: (onSoft = false): CardChip => ({
      key: "direct",
      label: t("providerCard.chip.directWhenDirect"),
      tone: onSoft ? "outline" : "direct",
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
          // 实际在路由 / 叠加，正在查看直连
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
            const current = p.id === routeId;
            if (p.id === directId) chips.push(chip.directWhenDirect(current));
            if (current) {
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
      // 故障转移开着：队列 + 不在队列
      const known = new Set(providers.map((p) => p.id));
      const queued = queue.filter((id) => known.has(id));
      const byId = new Map(providers.map((p) => [p.id, p]));
      const queueItems = queued.map((id, index) => {
        const p = byId.get(id)!;
        return {
          provider: p,
          presentation: {
            tone: index === 0 ? "route" : undefined,
            status:
              index === 0
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
                index < queued.length - 1
                  ? () => actions.queueMove(p, 1)
                  : undefined,
            },
          } satisfies CardPresentation,
        };
      });
      const rest = providers
        .filter((p) => !queued.includes(p.id))
        .map((p) => {
          if (blockedFromRouting(app, p))
            return blocked(p, t("providerCard.action.addToQueue"), noRoute);
          const chips = officialOnly(p);
          if (p.id === directId) chips.push(chip.directWhenDirect());
          return {
            provider: p,
            presentation: {
              chips,
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

    // 实际在直连 / 叠加，正在查看路由
    return [
      {
        key: "all",
        items: providers.map((p) => {
          if (blockedFromRouting(app, p))
            return blocked(p, t("providerCard.action.startRouteFrom"), noRoute);
          const chips = officialOnly(p);
          if (p.id === routeId)
            chips.push({
              key: "lastRoute",
              label: t("providerCard.chip.lastRoute"),
              tone: "route",
            });
          if (p.id === directId) chips.push(chip.directWhenDirect());
          return {
            provider: p,
            presentation: {
              chips,
              buttons: [
                {
                  key: "startRouteFrom",
                  label: t("providerCard.action.startRouteFrom"),
                  onClick: () => actions.startRouteFrom(p),
                },
              ],
            },
          };
        }),
      },
    ];
  }

  // view === "stack"
  const on = active === "stack";
  // 没进叠加时每行的主操作是「以这家为默认开始叠加…」（和路由预览的「从这家开始路由」对齐）；
  // 进了叠加后才是「设为默认」，当场生效
  const defaultButton = (p: Provider): CardButton =>
    on
      ? {
          key: "setDefault",
          label: t("providerCard.action.setDefault"),
          onClick: () => actions.stackSetDefault(p),
        }
      : {
          key: "startStackFrom",
          label: t("providerCard.action.startStackFrom"),
          onClick: () => actions.startStackFrom(p),
        };
  const defaultId = (() => {
    const candidate = on ? routeId : (routeId ?? directId);
    const p = providers.find((x) => x.id === candidate);
    return p && !blockedFromRouting(app, p) ? p.id : null;
  })();
  const defaultProvider = providers.find((p) => p.id === defaultId);
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
        buttons: [
          defaultButton(p),
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
            buttons: [
              defaultButton(p),
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
          buttons: [
            ...(on ? [] : [defaultButton(p)]),
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
        body: t("providerCard.section.stackDefaultHelp"),
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

// ─── Claude Desktop ─────────────────────────────────────────────────────────

export function buildDesktopSections({
  t,
  providers,
  currentId,
  onSwitch,
}: {
  t: TFunction;
  providers: Provider[];
  currentId: string;
  onSwitch: (provider: Provider) => void;
}): ProviderSection[] {
  return [
    {
      key: "all",
      items: providers.map((p) => {
        const mapping = providerNeedsRouting("claude-desktop", p);
        const chips: CardChip[] = [];
        if (isOfficialAccount("claude-desktop", p)) {
          chips.push({
            key: "official",
            label: t("providerCard.chip.official"),
            tone: "outline",
          });
        }
        const current = p.id === currentId;
        chips.push(
          mapping
            ? {
                key: "mapping",
                label: t("providerCard.chip.mapping"),
                tone: current ? "outline" : "route",
              }
            : {
                key: "direct",
                label: t("providerCard.chip.direct"),
                tone: current ? "outline" : "direct",
              },
        );
        if (current) {
          return {
            provider: p,
            presentation: {
              tone: mapping ? "route" : "direct",
              status: {
                label: t("providerCard.status.inUse"),
                dot: mapping ? "route" : "direct",
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
                onClick: () => onSwitch(p),
              },
            ],
          },
        };
      }),
    },
  ];
}

// ─── 共存式应用（OpenCode / OpenClaw / Hermes / Pi / MiniMax Code）───────────

export interface AdditiveInput {
  app: AppId;
  t: TFunction;
  providers: Provider[];
  isInConfig: (provider: Provider) => boolean;
  /** OpenCode：当前启用的 OMO / OMO Slim */
  currentOmoId?: string | null;
  currentOmoSlimId?: string | null;
  /** OpenClaw：默认模型属于哪家、它的模型名 */
  openclawDefault?: { providerId: string; model: string } | null;
  /** OpenClaw：每家可选的模型 */
  openclawModels?: (provider: Provider) => { id: string; name?: string }[];
  /** Hermes：model.provider 指向的那家 */
  hermesCurrentId?: string | null;
  isHermesManaged?: (provider: Provider) => boolean;
  /** Pi：读不到当前配置时一律不能改 */
  piStateUnavailable?: boolean;
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
    app === "pi" && input.piStateUnavailable
      ? t("pi.current.stateUnavailableHint")
      : undefined;

  for (const p of providers) {
    const chips: CardChip[] = [];
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
              label:
                app === "pi"
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

    // 已添加：白底、不写「● 已添加」（分组标题和「移除」已经说清楚）；灰底只给默认那一家
    const buttons: CardButton[] = [];
    let status: CardPresentation["status"];
    let isDefault = false;
    let removeReason: string | undefined = piBlocked;

    if (app === "openclaw") {
      isDefault = input.openclawDefault?.providerId === p.id;
      if (isDefault) {
        status = { label: t("providerCard.status.default"), dot: "muted" };
        chips.push({
          key: "defaultModel",
          label: t("providerCard.chip.defaultModel", {
            model: input.openclawDefault?.model ?? "",
          }),
          tone: "outline",
        });
        removeReason = t("providerCard.reason.defaultCannotRemove");
      } else {
        const models = input.openclawModels?.(p) ?? [];
        buttons.push({
          key: "setDefault",
          label: t("providerCard.action.setDefault"),
          onClick: () => actions.setDefault(p, models[0]?.id),
          menu:
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

    if (app === "hermes") {
      if (p.id === input.hermesCurrentId) {
        isDefault = true;
        status = { label: t("providerCard.status.current"), dot: "muted" };
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
        tone: isDefault ? "neutral" : undefined,
        status,
        chips,
        dim: managed,
        editDisabledReason: readOnlyReason,
        deleteDisabledReason: readOnlyReason ?? piBlocked,
        buttons,
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
