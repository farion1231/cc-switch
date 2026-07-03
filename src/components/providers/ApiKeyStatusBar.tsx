import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Timer, AlertCircle, CheckCircle2, ZapOff, ChevronDown } from "lucide-react";
import type { UsageResult, UsageData } from "@/types";
import { usageApi } from "@/lib/api/usage";
import { cn } from "@/lib/utils";

/**
 * 单把 API Key 的状态条。
 *
 * 信息密度从下往上依次为：
 *   1. 进度条：填充宽度 = 账户用量 used%，颜色按阈值变红/黄/灰
 *   2. 进度条右侧元数据：百分比 + 7d 窗口标签 + used/total 数字
 *   3. 进度条上方一行：冷却倒计时（若在 cooldown）——用 `useState` 每秒 tick，
 *      否则渲染一次后即停
 *
 * 设计原则：
 *   - 一把 key 一行信息，绝不与同行其它 key 抢空间（视觉权重比电池徽标更重）
 *   - 没用 usage_script 的 provider：仅显示冷却倒计时，progress 部分直接不渲染
 *   - 「7d」是显示文案锚点——绝大多数 provider 的用量配额都是 7 天滚动窗口；
 *     即便后端没传 resets_at，UI 也能给用户一个稳定的"7 天"心智模型
 */
interface ApiKeyStatusBarProps {
  /** 这把 key 的 id；传给 backend 触发 proactive rotation 时使用 */
  keyId?: string | null;
  /** epoch seconds，同 ApiKeyDto.cooldownUntil */
  cooldownUntil: number;
  /** 累计失败次数（>0 时即便未在冷却也要显示，给用户一个"距停用还有多远"的锚点） */
  failureCount?: number;
  /** key 是否启用（被自动停用或手动停用时显示对应文案） */
  enabled?: boolean;
  /** 触发自动停用的失败阈值，UI 文案用 */
  autoDisableThreshold?: number;
  /** 账户级别 usage 数据；未传或 success=false 时不渲染 usage 部分 */
  usage?: UsageResult | null;
  /** 是否启用 usage 展示（provider 是否配了 usage_script） */
  usageEnabled?: boolean;
  className?: string;
  /**
   * 5h 配额窗口倒计时归零时的回调。父组件接到回调后应主动 invalidate
   * keyUsage / apiKeys / usage 等 query，让前端 UI 立即拉到「重置后」
   * 的新快照，而不必等到下一个 autoQueryInterval 周期。
   *
   * - 该回调在 primaryResetMs 倒计到 0 时触发，且每次重置后会在新的
   *   primaryResetMs 上重新挂 timer（依赖数组含 primaryResetMs）。
   * - 父组件可以用 `["apiKeys", providerId, appId]` 失效来顺带刷新
   *   冷却 / 失败计数 / enabled 标志。
   * - 该回调可能与 React Query 的 `refetchInterval` 同时触发——重复
   *   invalidate 是幂等的，不会引发额外请求。
   */
  onFiveHourResetReached?: () => void;
  /**
   * 当任一 quota 达到 100%（5h / 7d / 任何 tier）时回调。父组件用这个
   * 给整行加红色背景——7d 100% 比 5h 100% 更值得警示，但用户视角里
   * 「这把 key 已经耗尽」是同一个语义，单一信号最稳。
   * 7d 100% 但额外 tier 列表未渲染的场景下，让 row 自行染红
   * 是更可靠的可见反馈，避免「bar 不可见 = 信号丢失」。
   */
  onExhaustedChange?: (exhausted: boolean) => void;
  /**
   * 折叠状态：默认 `false`（状态行 + 进度条全展开，与旧版一致）。
   * 设为 `true` 时只展示顶部的状态行（冷却 / 停用 / 失败次数 / 就绪），
   * 5h / 7d 用量进度条藏在点击背后——用于「同一把 key 在编辑表单里
   * 出现 N 次时，表单已经很高，不要每个 row 都强制展开两层进度条」的
   * 场景。KeyPoolBadge 不传该 prop，行为不变。
   */
  defaultCollapsed?: boolean;
}

export function ApiKeyStatusBar({
  keyId,
  cooldownUntil,
  failureCount = 0,
  enabled = true,
  autoDisableThreshold = 5,
  usage,
  usageEnabled = false,
  className,
  onFiveHourResetReached,
  onExhaustedChange,
  defaultCollapsed = false,
}: ApiKeyStatusBarProps) {
  const { t } = useTranslation();
  // 上一次触发 proactive rotation 的「区间桶」——跨档位（safe→warn→
  // exhausted）才重新触发，90→95 的连续刷新不会重复打 backend。
  const lastTriggeredBucketRef = useRef<"warn" | "exhausted" | null>(null);

  // ─── 折叠状态 ─────────────────────────────────────────────
  // 状态行始终可见（冷却倒计时 / 停用 / 失败次数 / 就绪——这是用户
  // 必看的「这把 key 现在啥状态」锚点）。进度条区（5h 主条 + 副 tier
  // 列表）在 defaultCollapsed=true 时收起来，用户点状态行再展开。
  const [expanded, setExpanded] = useState(!defaultCollapsed);

  // ─── 冷却倒计时 ─────────────────────────────────────────────
  // 每秒 tick 一次；冷却结束后清理 interval，避免不必要 re-render。
  // useState 的初始化器只在挂载时跑一次，所以额外加一个 effect 同步 prop
  // 变化（轮换恢复时 cooldownUntil 会被后端改写，需要重新计算剩余）。
  const computeRemaining = () =>
    Math.max(0, cooldownUntil * 1000 - Date.now());
  const [remainingMs, setRemainingMs] = useState(computeRemaining);
  const inCooldown = remainingMs > 0;

  useEffect(() => {
    setRemainingMs(computeRemaining());
  }, [cooldownUntil]);

  useEffect(() => {
    if (!inCooldown) return;
    const id = setInterval(() => {
      const r = computeRemaining();
      setRemainingMs(r);
      if (r === 0) clearInterval(id);
    }, 1000);
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [inCooldown, cooldownUntil]);

  // ─── 用量数据 ───────────────────────────────────────────────
  // usage 是 per-key 维度的——每把 key 由 KeyPoolList 的 useQueries
  // 单独拉（queryProviderUsageForKey 使用 provider_api_keys 里这一行
  // 自己的 api_key 跑脚本）。所以进度条数字精确归到对应的那把 key 上，
  // 不再像之前那样所有 key 共享 provider-level 快照。
  //
  // **进度条始终渲染**（只要 `usageEnabled` 为 true）——即便当前还没拉到
  // 数据（loading / 瞬时失败）也要让用户看到"这里有个用量条"的位置锚点，
  // 不会因为一次 query 抖动就让整行"空"得让人误以为没配 usage_script。
  // 拉到数据时按数据走；没拉到时用 0% 占位。
  // 进度条顶部那条主用「5h 配额」显示——挑 planName 标识为 5 小时的那条；
  // 若 usage_script 返回的多 plan 里没有显式 5h 标识，再回退到 data[0]。
  // 关键：data[0] 未必是 5h（7天窗口的 plan 经常排在前面，used 数字会差
  // 一个数量级），拿它去标「5h 配额」会让用户看错数字。
  // 例：5h plan 100% / 7d plan 86% 时，data[0] 拿的是 7d，标 5h 就是误导。
  // —— 还有一种更隐蔽的情况：脚本只返回 1 个 UsageData 元素，把 5h / 7d 两个
  // 窗口都塞到 `extra` 的「5小时:100%12m 7天:86%12m」里。这时 find() 找不到
  // 5h planName，会落到 data[0]，而 data[0].used 反映的可能是 7d 数字（脚本
  // 实现不统一）。补救：从 `extra` 解析出的 usageWindows 里挑 5h 的那条用作
  // `usedPercent` / `used` / `total`，优先于 data[0] 的数字。
  // 5h / 7d 正则提前到外层作用域，让下面 primaryForDisplay 也能复用。
  // 注意：planName 在不同数据源里长得不一样：
  //   - subscription 路径（SubscriptionQuota → UsageData 映射）：tier.name
  //     写的是内部 key "five_hour" / "seven_day" / "seven_day_opus" 等；
  //   - JS 脚本路径：通常写展示文案 "7天配额" / "周配额" / "Weekly"；
  //   - 旧 coding_plan 模板回退：也用 "7d 配额" / "5h 配额" 这种简短标签。
  // 一并匹配这三种形态——否则会把"7d 配额"标签贴到 5h 数据上。
  // 5h 正则：覆盖 "five_hour" / "5h" / "5小时" / "5小时配额" 等所有形态。
  // 主进度条优先选 5h（短期窗口对单次会话的影响更直接），7d 退到下方
  // 「per-window 进度条」或「多 tier 列表」里展示。
  const fiveHourRe = /(5\s*h(?:our)?s?|5\s*小时|five_hour)/i;
  // 7d 正则：多 tier 列表 / windowsForList 的 dual-format 决策需要——
  // 名字匹配 7d → 主单位是「天」（"3d20h"），匹配 5h → 主单位是「小时」
  // （"1h54m"），未知窗口默认 7d 行为。
  const sevenDayRe = /(7\s*d(?:ay)?s?|7\s*天|周|week|seven_day)/i;
  const primaryUsage: UsageData | null = (() => {
    if (
      !usageEnabled ||
      !usage?.success ||
      !usage.data ||
      usage.data.length === 0
    ) {
      return null;
    }
    const fiveHour = usage.data.find((d) => {
      const name = (d.planName ?? "").trim();
      return name && fiveHourRe.test(name);
    });
    return fiveHour ?? usage.data[0];
  })();
  // 解析 primaryUsage.extra 中的多窗口配额段，**提前**到这里，以便下面的
  // 百分比 / used 计算能参考 5h 窗口的真实数字。
  const usageWindows = primaryUsage ? parseUsageWindows(primaryUsage.extra) : [];
  // 当 primaryUsage 自身没有 5h 标识（被回退到 data[0]）时，从 usageWindows
  // 里挑 5h 窗口，构造一个临时 UsageData 用于主进度条的数字——避免"标签
  // 5h 配额"与"数字来自 7d"的割裂。这只覆盖 used/usedPercent/unit；planName
  // 留空（因为它本身就是用 extra 编码的）。
  const fiveHourWindow = usageWindows.find((w) => fiveHourRe.test(w.name));
  const primaryForDisplay: UsageData | null = (() => {
    if (!primaryUsage) return null;
    if (fiveHourWindow) {
      // primaryUsage 已有 5h 标识 → 用 primaryUsage；
      // 没有 5h 标识但 extra 里能解析出 5h 窗口 → 用 5h 窗口的数字。
      const hasFiveHourName = fiveHourRe.test(
        (primaryUsage.planName ?? "").trim(),
      );
      if (!hasFiveHourName) {
        return {
          ...primaryUsage,
          // 5h 窗口没有原始 used 绝对值含义，用 percent 当 used（unit="%"）
          used: fiveHourWindow.used,
          unit: "%",
          total: 100,
        };
      }
    }
    return primaryUsage;
  })();
  const usedPercent =
    primaryForDisplay && typeof primaryForDisplay.used === "number"
      ? (() => {
          // unit 决定 used 的语义：
          //   - "%" → used 已是 0–100 的百分比，直接用
          //   - 其他（"count" / "tokens" / undefined 但 total>0）→ used/total
          //     重新换算成 0–100，否则进度条永远停在 7% 这种「绝对值小」的位置上
          if (primaryForDisplay.unit === "%") {
            return Math.max(0, Math.min(100, primaryForDisplay.used));
          }
          const total = primaryForDisplay.total ?? 0;
          if (total > 0) {
            return Math.max(
              0,
              Math.min(100, (primaryForDisplay.used / total) * 100),
            );
          }
          return 0;
        })()
      : 0;
  const showUsageBar = usageEnabled;

  // 解析 primaryUsage.extra 中的多窗口配额段已经在上面完成（usageWindows），
  // 5h 窗口拣选后已并入 primaryForDisplay 用于主进度条的数字；下方多窗口
  // 进度条区继续用 usageWindows 渲染。
  // —— 当 5h 窗口已被挑出来作为 primaryForDisplay 的来源时，从 usageWindows
  // 里移除它，避免在「主进度条 100%」+「下方 5小时:100% 进度条 100%」双重复。
  const windowsForList = fiveHourWindow
    ? usageWindows.filter((w) => w !== fiveHourWindow)
    : usageWindows;

  // 失败次数在展示层封顶——5 次就是自动停用 + 轮换的硬阈值，再往后累
  // 加已经没有信息量（用户已经知道"这把废了"）。底层 backend 仍然会继续
  // 累加（用于审计/重启用时的初始值），但 UI 不展示超过 5 的数字，避免
  // 「已失败 17/5 次」这种让用户怀疑阈值的观感。
  const displayedFailureCount = Math.min(
    Math.max(0, failureCount),
    autoDisableThreshold,
  );

  // 主进度条的「重置倒计时」——7d/30d 那些副 tier 在 windowsForList 里用
  // 「5h:100%12m」字符串里的 "12m" 段当 reset 倒计时，5h 主进度条因为被
  // 抽出来复用数字，反而丢了 reset。这里补回：
  //   - 5h 主数据来自 `fiveHourWindow`（字符串编码的「5小时:100%12m」）：
  //     reset 直接是 `fiveHourWindow.reset`
  //   - 5h 主数据来自 subscription 风格的 UsageData（planName="five_hour"、
  //     extra 是 ISO resetsAt）：用 parseResetsAtForBar 解析 ISO
  //   - 上面两种都没有：fallback 到「未知」
  // 把"5h 配额"标签后面挂一个倒计时，跟下面 7d 行的 7d 倒计时视觉一致，
  // 用户能横向对照「5h 还多久 / 7d 还多久」。
  //
  // 同时把 reset 字符串还原回毫秒——下面 `primaryResetLabel` 用「XhYm」/
  // 「XdYh」双单位格式重打，"12m"/"2h" 这种单单位粒度太粗，主进度条不
  // 该用这种简化版。
  const primaryReset: string = (() => {
    if (fiveHourWindow?.reset) return fiveHourWindow.reset;
    if (primaryForDisplay) {
      const isoReset = parseResetsAtForBar(primaryForDisplay.extra);
      if (isoReset) return isoReset;
    }
    return "";
  })();
  // `parseResetsAtForBar` 拿到的是 ISO 串走 formatRemaining 出来的紧凑
  // 字符串（"12m" / "2h" / "3d"），需要再 `parseCompactDurationToMs` 回到
  // ms 才能喂给 `formatDualCountdown` 走双单位分支。
  // `fiveHourWindow.reset` 同源（"5小时:100%12m" 里的 "12m"）—— 同样路径。
  const primaryResetMs = primaryReset ? parseCompactDurationToMs(primaryReset) : 0;

  // ─── 5h 配额窗口到点重置 ───────────────────────────────────
  // primaryResetMs 是「距下次重置的剩余 ms」——挂一个 setTimeout，到点时
  // 调 onFiveHourResetReached 让父组件 invalidate 相关 query。
  //
  // 设计要点：
  //   - 依赖 primaryResetMs：每次重置后新数据里 primaryResetMs 会变成
  //     「新的 5h 窗口」剩余时长（≈ 5h），effect 重跑、自然续约。
  //   - cleanup：return 出来的 clearTimeout 在 unmount 或 primaryResetMs
  //     变化时清掉旧 timer，避免「快进到下一次 invalidate」时叠两个
  //     timer 导致 onFiveHourResetReached 被调多次。
  //   - 100ms 下限：primaryResetMs 刚到 0 附近（数据已显示 0m 但新
  //     snapshot 还没到）时挂个 0ms 定时器会让 setTimeout 立刻 fire，
  //     给后端一个喘息时间避免 throttle；并且 primaryResetMs 为 0
  //     （数据缺失）时不挂 timer，由 UI 显示「未知」即可。
  //   - onFiveHourResetReached 进依赖：父组件的 handleKeyResetReached
  //     通常是稳定的（闭包持有 queryClient / providerId / appId），
  //     但保险起见还是放进去，避免 hook 引用陈旧。
  useEffect(() => {
    if (!onFiveHourResetReached || primaryResetMs <= 0) return;
    const delay = Math.max(primaryResetMs, 100);
    const timer = setTimeout(onFiveHourResetReached, delay);
    return () => clearTimeout(timer);
  }, [primaryResetMs, onFiveHourResetReached]);

  // ─── Proactive rotation ────────────────────────────────────
  // 监听 5h 主窗口的 usage_percent，超过阈值时通知 backend KeyRing
  // 把这把 key 提前送进 cooldown——避免「先发请求被拒再切 key」的延迟。
  //
  // 设计要点：
  //   - **节流**：lastTriggeredBucketRef 记录上一次触发的「区间桶」
  //     （warn / exhausted / safe），跨档位时再触发，90→95 不会重复
  //     触发 N 次，但 80→90 仍会触发一次。
  //   - **reset_at 来自 primaryResetMs**：当 cooldown_until = reset_at，
  //     5h 窗口自然重置后（usagePercent 数据更新到 0%），下一次 effect
  //     触发时 backoff 到 safe 桶——key 自动恢复。
  //   - **不 await**：usageApi.markKeyUsageHigh 内部已经 swallow 异常，
  //     这里是 fire-and-forget，UI 不会因 backend 错误而卡顿。
  useEffect(() => {
    if (!keyId || !primaryForDisplay) return;
    const used = primaryForDisplay.used ?? 0;
    const total = primaryForDisplay.total ?? 100;
    if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0) {
      return;
    }
    const usagePercent = (used / total) * 100;
    const resetAtUnix =
      primaryResetMs > 0 ? Math.floor(Date.now() / 1000) + Math.floor(primaryResetMs / 1000) : 0;
    let bucket: "safe" | "warn" | "exhausted" = "safe";
    if (usagePercent >= 100) bucket = "exhausted";
    else if (usagePercent >= 90) bucket = "warn";
    if (bucket === "safe") {
      // 触底后清除之前记录的桶——下次再涨过阈值还能再触发。
      lastTriggeredBucketRef.current = null;
      return;
    }
    if (lastTriggeredBucketRef.current === bucket) return;
    lastTriggeredBucketRef.current = bucket;
    void usageApi.markKeyUsageHigh(keyId, usagePercent, resetAtUnix);
  }, [keyId, primaryForDisplay, primaryResetMs]);

  // 订阅/Subscription-style 用量（TOKEN_PLAN 走 special-template 路径）以
  // `usage.data[]` 形式返回多个 `UsageData`——每个 element 是一个 tier
  // （planName = "five_hour" / "seven_day" / "monthly" / "weekly_limit"），
  // `extra` 里是 ISO `resetsAt`。这些不在 `parseUsageWindows` 解析的
  // 「5小时:100%12m」格式里，所以不靠 `windowsForList` 渲染；单独列一份
  // 「非主进度条 tier 列表」，把 5h 等一并展示——否则 per-key 行只会显示 7d
  // 一条，5h 永远缺失，与用户预期「5h + 7d 两条都看得到」不符。
  const additionalTiers: Array<{
    name: string;
    used: number;
    reset: string;
  }> = [];
  if (usageEnabled && usage?.success && usage.data && primaryUsage) {
    for (const d of usage.data) {
      const name = (d.planName ?? "").trim();
      if (!name) continue;
      if (name === (primaryUsage.planName ?? "").trim()) continue;
      const used =
        d.unit === "%"
          ? Math.max(0, Math.min(100, d.used ?? 0))
          : d.total && d.total > 0
            ? Math.max(0, Math.min(100, ((d.used ?? 0) / d.total) * 100))
            : 0;
      const reset = parseResetsAtForBar(d.extra);
      additionalTiers.push({ name, used, reset });
    }
  }

  // 任一 quota 达到 100% → row 染红。涵盖：
  //   - 主进度条 usedPercent（5h 或 7d 取决于 primaryUsage 选择）
  //   - 多窗口 windowsForList（encoded 「7d:100%12m」）
  //   - 订阅风格 additionalTiers（planName="seven_day" 等）
  // 单一信号给 row，弱化「bar 不可见 = 信号丢失」的风险（参见
  // onExhaustedChange 注释）。
  const anyQuotaExhausted =
    usedPercent >= 100 ||
    windowsForList.some((w) => w.used >= 100) ||
    additionalTiers.some((t) => t.used >= 100);

  // 任一 quota 100% → 上报父组件 row 染红。依赖里包 anyQuotaExhausted
  // 和 onExhaustedChange；onExhaustedChange 一般稳定（闭包持有 queryClient
  // 等），但保险起见放进去避免 hook 引用陈旧。
  useEffect(() => {
    onExhaustedChange?.(anyQuotaExhausted);
  }, [anyQuotaExhausted, onExhaustedChange]);

  // ─── 是否存在可折叠内容 ─────────────────────────────────────
  // 状态行永远渲染（冷却 / 停用 / 失败 / 就绪——这是核心状态信号）。
  // 进度条 + 多 tier 列表只在「确实存在」时折叠；都不存在时整体
  // 没东西可展开，把整个 status row 设成不可点击，避免一个点了
  // 啥都不发生的按钮。
  const hasCollapsibleContent =
    usageEnabled ||
    windowsForList.length > 0 ||
    additionalTiers.length > 0;

  // ─── 标签：把"周配额" / "5h 配额" 等不同来源的 planName 收口到 i18n ──
  // SubscriptionQuota → UsageData 映射里写的是内部 key（"five_hour" /
  // "seven_day" / "seven_day_opus" / "weekly_limit"），JS 脚本路径写的是
  // 展示文案（"7天配额" / "周配额" / "Weekly"），二者必须分流，否则 "7d 配额"
  // 这种静态标签会贴在 5h 数据上误导用户。计算放在 primaryForDisplay 之后。
  const TIER_I18N_KEYS: Record<string, string> = {
    five_hour: "subscription.fiveHour",
    seven_day: "subscription.sevenDay",
    seven_day_opus: "subscription.sevenDayOpus",
    seven_day_sonnet: "subscription.sevenDaySonnet",
    weekly_limit: "subscription.sevenDay",
  };
  const planLabel = (() => {
    const raw = primaryForDisplay?.planName?.trim();
    if (!raw) return t("apiKeyStatusBar.fiveHour", { defaultValue: "5h 配额" });
    if (raw in TIER_I18N_KEYS) {
      return t(TIER_I18N_KEYS[raw], { defaultValue: raw });
    }
    return raw;
  })();

  // 永远渲染——用户能直观看到「就绪 / 失败计数 / 停用 / 冷却中」四种状态
  // 之一，不希望「什么都没出现」让人误以为 key 没在管。

  // 调试用：把每个 keyId 的最新 usage 数据挂到 window 上，方便
  // DevTools 直接 inspect——比在 Rust 日志里翻 trace 直观。
  //   window.__apiKeyUsage[keyId] = { usage, primaryUsage, additionalTiers,
  //     windowsForList, usedPercent, anyQuotaExhausted }
  // 仅开发期使用——上生产前会删。useEffect 在 data 真正变化时触发，
  // 不在每帧 render 上 set 新对象。
  useEffect(() => {
    if (typeof window === "undefined") return;
    (window as unknown as { __apiKeyUsage?: Record<string, unknown> }).__apiKeyUsage =
      {
        ...((window as unknown as { __apiKeyUsage?: Record<string, unknown> })
          .__apiKeyUsage ?? {}),
        [keyId as string]: {
          usage,
          primaryUsage,
          primaryForDisplay,
          usageWindows,
          windowsForList,
          additionalTiers,
          usedPercent,
          anyQuotaExhausted,
        },
      };
  }, [
    keyId,
    usage,
    primaryUsage,
    primaryForDisplay,
    usageWindows,
    windowsForList,
    additionalTiers,
    usedPercent,
    anyQuotaExhausted,
  ]);

  return (
    <div
      className={cn(
        "mt-2 space-y-1 text-[10px] text-muted-foreground",
        className,
      )}
    >
      {/* 状态行：四种状态互斥——冷却中 / 已停用 / 失败计数 / 就绪
          始终渲染（只要有任意信息可显示），给用户一个稳定的"这把 key 现在
          是什么状态"锚点。即便没有 usage 数据，也要让用户看到冷却 / 失败
          / 停用状态——这正是上一版没做对的地方。

          当存在可折叠内容（主进度条 / 副 tier）时，整行变成可点击的折叠
          触发器——点 status 行展开/收起下方进度条，避免「一个 row 永远顶
          着 3 行进度条」撑高编辑表单。type="button" 防止误提交外层 form
          （与 ApiKeyListSection 里其它按钮同源处理）。 */}
      <div className="flex items-center gap-1">
        <button
          type="button"
          onClick={
            hasCollapsibleContent ? () => setExpanded((v) => !v) : undefined
          }
          aria-expanded={hasCollapsibleContent ? expanded : undefined}
          className={cn(
            "flex flex-1 items-center gap-2 text-left",
            hasCollapsibleContent
              ? "cursor-pointer hover:text-foreground/90"
              : "cursor-default",
          )}
        >
          {inCooldown ? (
            <>
              <Timer className="h-3 w-3" />
              <span className="font-medium">
                {t("apiKeyStatusBar.cooldownRemaining", {
                  defaultValue: "冷却剩余",
                })}
              </span>
              <span className="font-mono tabular-nums text-foreground/80">
                {localizeCompactDuration(formatRemaining(remainingMs), t)}
              </span>
              <span className="text-muted-foreground/60">
                {t("apiKeyStatusBar.cooldownHint", {
                  defaultValue: "(届时自动恢复轮换)",
                })}
              </span>
            </>
          ) : !enabled ? (
            <>
              <ZapOff className="h-3 w-3" />
              <span className="font-medium">
                {t("apiKeyStatusBar.disabled", {
                  defaultValue: "已停用",
                })}
              </span>
              <span className="text-muted-foreground/60">
                {t("apiKeyStatusBar.disabledHint", {
                  defaultValue: "(手动启用后恢复轮换)",
                })}
              </span>
            </>
          ) : failureCount > 0 ? (
            <>
              <AlertCircle className="h-3 w-3" />
              <span className="font-medium">
                {t("apiKeyStatusBar.failureCount", {
                  defaultValue: `已失败 ${displayedFailureCount} / ${autoDisableThreshold} 次`,
                  count: displayedFailureCount,
                  threshold: autoDisableThreshold,
                })}
              </span>
              <span className="text-muted-foreground/60">
                {t("apiKeyStatusBar.failureHint", {
                  defaultValue: `(再失败 ${autoDisableThreshold - displayedFailureCount} 次将自动停用)`,
                  remaining: autoDisableThreshold - displayedFailureCount,
                })}
              </span>
            </>
          ) : (
            <>
              <CheckCircle2 className="h-3 w-3" />
              <span className="font-medium">
                {t("apiKeyStatusBar.ready", {
                  defaultValue: "冷却状态：就绪",
                })}
              </span>
              <span className="text-muted-foreground/60">
                {t("apiKeyStatusBar.readyHint", {
                  defaultValue: `(${autoDisableThreshold} 次失败后自动停用)`,
                  threshold: autoDisableThreshold,
                })}
              </span>
            </>
          )}
        </button>
        {hasCollapsibleContent && (
          <ChevronDown
            className={cn(
              "h-3 w-3 shrink-0 text-muted-foreground/60 transition-transform duration-150",
              expanded && "rotate-180",
            )}
            aria-hidden="true"
          />
        )}
      </div>

      {/* 进度条行 */}
      {expanded && showUsageBar && (
        <div className="space-y-0.5">
          {(() => {
            // 动态上限：coding plan 有时 100% 实际只算"基础套餐"，
            // 真实封顶可能在 150% / 200%——按数据实际峰值动态上浮 aria-valuemax
            // 让 a11y 报告真实比例。视觉宽度仍 clamp 到 100% 以免溢出；
            // 颜色用 100% 阈值，与原版阈值一致。
            const ariaMax = Math.max(100, usedPercent ?? 0);
            return (
              <div
                className="flex items-center gap-1.5"
                role="presentation"
              >
                <div
                  className="relative h-1.5 flex-1 overflow-hidden rounded-full bg-muted"
                  role="progressbar"
                  aria-valuenow={usedPercent ?? 0}
                  aria-valuemin={0}
                  aria-valuemax={ariaMax}
                >
                  <div
                    className={cn(
                      "h-full rounded-full transition-all duration-500 ease-out",
                      usedPercent !== null && usedPercent >= 90
                        ? "bg-red-500"
                        : usedPercent !== null && usedPercent >= 70
                          ? "bg-amber-500"
                          : "bg-blue-500",
                    )}
                    // 视觉宽度 clamp 到 100%——>100% 的超额场景由右侧
                    // 文字与 red 颜色表达，不让 bar 溢出容器。
                    style={{ width: `${Math.min(100, usedPercent ?? 0)}%` }}
                  />
                  {/* 70% / 90% 阈值标记线：让用户直观看到离上限还有多远 */}
                  <div className="pointer-events-none absolute inset-y-0 left-[70%] w-px bg-foreground/15" />
                  <div className="pointer-events-none absolute inset-y-0 left-[90%] w-px bg-foreground/20" />
                </div>
                <span
                  className={cn(
                    "min-w-[3ch] text-right font-mono tabular-nums",
                    usedPercent !== null && usedPercent >= 90
                      ? "text-red-500"
                      : usedPercent !== null && usedPercent >= 70
                        ? "text-amber-500"
                        : "text-foreground/80",
                  )}
                >
                  {usedPercent}%
                </span>
              </div>
            );
          })()}
          <div className="flex items-center gap-2 text-muted-foreground/80">
            {/* 5h / 7d 标签——以前这里有一个 CalendarClock 计划表图标，
                跟"配额窗口"含义重复（label 本身就说明是周期）。删掉
                让文字本身承担语义，视觉更干净。Timer 图标仍保留给
                右侧的 reset 倒计时——"时间在跑"是它独有的含义。 */}
            <span>{planLabel}</span>
            {/* 绝对数字 used/total 和「剩 X%」与进度条右侧的 usedPercent%
                是同一份数据的不同呈现——进度条本身已带百分比数字，再把
                "24 / 100" 和 "剩 76%" 横排一次会变成 N+1 行同源信息，
                信息密度低，挤掉「5h 配额 24%」和「重置倒计时」的对比空间。
                改后只保留 planLabel + 重置倒计时；想看绝对数字可 hover
                进度条（aria-valuenow/title 已透出 used/total）。 */}
            {primaryResetMs > 0 && (
              <span
                className={cn(
                  "ml-auto inline-flex items-center gap-0.5 font-mono tabular-nums",
                  "text-foreground/70",
                )}
                title={t("apiKeyStatusBar.windowResetsIn", {
                  defaultValue: `将在 ${primaryReset} 后重置`,
                  reset: primaryReset,
                })}
              >
                <Timer className="h-2.5 w-2.5" />
                {localizeCompactDuration(
                  formatDualCountdown(primaryResetMs, "5h"),
                  t,
                )}
              </span>
            )}
          </div>
        </div>
      )}

      {/* Per-window 倒计时进度条。
          解析 UsageData.extra 里的「5小时:100%12m / 7天:86%12m」格式——
          这类「window:used%<reset>」段被多个 provider（MiniMax / Packy 等）用作
          多窗口配额展示。把它们拆成独立的进度条，比一行纯文字「12m」直观得多：
          填充宽度对应 used%、色阶按阈值变红/黄/蓝、右侧 reset 用紧凑的 Xs/Xm/Xh/Xd。
          解析失败时（provider 不输出这个格式）整段不渲染——不影响现有 fallback。
          注意：5h 窗口若已被上面 primaryForDisplay 吸收（避免与主进度条重复），
          这里就只列 7d / 30d / 月度等其它窗口。 */}
      {expanded && windowsForList.length > 0 && (
        <div className="mt-1.5 space-y-1.5">
          {windowsForList.map((w, i) => {
            // 双单位格式：窗口名匹配 5h/7d/30d → 用对应主单位分支；
            // 未知窗口（custom / monthly 等）默认按 7d 处理——主单位是「天」。
            const horizon: "5h" | "7d" | "30d" = fiveHourRe.test(w.name)
              ? "5h"
              : sevenDayRe.test(w.name)
                ? "7d"
                : "30d";
            const wResetMs = parseCompactDurationToMs(w.reset);
            const wResetLabel =
              wResetMs > 0
                            ? localizeCompactDuration(
                                formatDualCountdown(wResetMs, horizon),
                                t,
                              )
                            : w.reset;
            return (
              <div key={i} className="space-y-0.5">
                <div className="flex items-center gap-1.5">
                  <div
                    className="relative h-1 flex-1 overflow-hidden rounded-full bg-muted"
                    role="progressbar"
                    aria-valuenow={Math.round(w.used)}
                    aria-valuemin={0}
                    aria-valuemax={Math.max(100, Math.round(w.used))}
                  >
                    <div
                      className={cn(
                        "h-full rounded-full transition-all duration-500 ease-out",
                        w.used >= 90
                          ? "bg-red-500"
                          : w.used >= 70
                            ? "bg-amber-500"
                            : "bg-blue-500",
                      )}
                      style={{ width: `${Math.max(0, Math.min(100, w.used))}%` }}
                    />
                    <div className="pointer-events-none absolute inset-y-0 left-[70%] w-px bg-foreground/15" />
                    <div className="pointer-events-none absolute inset-y-0 left-[90%] w-px bg-foreground/20" />
                  </div>
                  <span
                    className={cn(
                      "min-w-[3ch] text-right font-mono tabular-nums text-[10px]",
                      w.used >= 90
                        ? "text-red-500"
                        : w.used >= 70
                          ? "text-amber-500"
                          : "text-foreground/70",
                    )}
                  >
                    {Math.round(w.used)}%
                  </span>
                </div>
                <div className="flex items-center justify-between gap-2 text-[10px] text-muted-foreground/80">
                  <span className="truncate">
                    {w.name in TIER_I18N_KEYS
                      ? t(TIER_I18N_KEYS[w.name], { defaultValue: w.name })
                      : labelForWindowName(w.name, t)}
                  </span>
                  <span
                    className={cn(
                      "inline-flex items-center gap-0.5 font-mono tabular-nums",
                      "text-foreground/70",
                    )}
                    title={t("apiKeyStatusBar.windowResetsIn", {
                      defaultValue: `将在 ${w.reset} 后重置`,
                      reset: w.reset,
                    })}
                  >
                    <Timer className="h-2.5 w-2.5" />
                    {wResetLabel}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* 多 tier（TOKEN_PLAN 风格：5h / 7d 各自一个 UsageData 元素）。
          上面 windowsForList 解析的是「5小时:100%12m」字符串；这里是
          usage.data[] 里非主 tier 元素——planName = "five_hour" / "monthly" /
          "weekly_limit" 等，extra 是 ISO resetsAt。与 windowsForList 用同一
          个紧凑行 UI（progress + label + reset），避免两套样式分裂。 */}
      {expanded && additionalTiers.length > 0 && (
        <div className="mt-1.5 space-y-1.5">
          {additionalTiers.map((w, i) => {
            // 双单位格式决策——和 windowsForList 同源逻辑：
            // 5h 名称 → "XhYm"，7d 名称 → "XdYh"，未知 → 默认 7d。
            // 这里用 i18n key 前（raw name）做正则匹配，因为
            // TIER_I18N_KEYS 里的翻译后字符串（如「5小时配额」）反而
            // 不一定能被 /5\s*小时/ 命中——不同语言下字面量会变。
            const horizon: "5h" | "7d" | "30d" = fiveHourRe.test(w.name)
              ? "5h"
              : sevenDayRe.test(w.name)
                ? "7d"
                : "30d";
            const wResetMs = parseCompactDurationToMs(w.reset);
            const wResetLabel =
              wResetMs > 0
                            ? localizeCompactDuration(
                                formatDualCountdown(wResetMs, horizon),
                                t,
                              )
                            : w.reset;
            return (
              <div key={`${w.name}-${i}`} className="space-y-0.5">
                <div className="flex items-center gap-1.5">
                  <div
                    className="relative h-1 flex-1 overflow-hidden rounded-full bg-muted"
                    role="progressbar"
                    aria-valuenow={Math.round(w.used)}
                    aria-valuemin={0}
                    aria-valuemax={Math.max(100, Math.round(w.used))}
                  >
                    <div
                      className={cn(
                        "h-full rounded-full transition-all duration-500 ease-out",
                        w.used >= 90
                          ? "bg-red-500"
                          : w.used >= 70
                            ? "bg-amber-500"
                            : "bg-blue-500",
                      )}
                      style={{ width: `${Math.max(0, Math.min(100, w.used))}%` }}
                    />
                    <div className="pointer-events-none absolute inset-y-0 left-[70%] w-px bg-foreground/15" />
                    <div className="pointer-events-none absolute inset-y-0 left-[90%] w-px bg-foreground/20" />
                  </div>
                  <span
                    className={cn(
                      "min-w-[3ch] text-right font-mono tabular-nums text-[10px]",
                      w.used >= 90
                        ? "text-red-500"
                        : w.used >= 70
                          ? "text-amber-500"
                          : "text-foreground/70",
                    )}
                  >
                    {Math.round(w.used)}%
                  </span>
                </div>
                <div className="flex items-center justify-between gap-2 text-[10px] text-muted-foreground/80">
                  <span className="truncate">
                    {w.name in TIER_I18N_KEYS
                      ? t(TIER_I18N_KEYS[w.name], { defaultValue: w.name })
                      : labelForWindowName(w.name, t)}
                  </span>
                  {w.reset ? (
                    <span
                      className={cn(
                        "inline-flex items-center gap-0.5 font-mono tabular-nums",
                        "text-foreground/70",
                      )}
                      title={t("apiKeyStatusBar.windowResetsIn", {
                        defaultValue: `将在 ${w.reset} 后重置`,
                        reset: w.reset,
                      })}
                    >
                      <Timer className="h-2.5 w-2.5" />
                      {wResetLabel}
                    </span>
                  ) : (
                    <span />
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ─────────── helpers ───────────

/** 5h 正则：模块级复用，避免 labelForWindowName 重复定义 */
const FIVE_HOUR_RE = /(5\s*h(?:our)?s?|5\s*小时|five_hour)/i;
/** 7d 正则：模块级复用 */
const SEVEN_DAY_RE = /(7\s*d(?:ay)?s?|7\s*天|周|week|seven_day)/i;

/**
 * JS-script-style 窗口名（"5小时" / "7天" / "30天" / "月配额" 等）的 i18n
 * 翻译。subscription-style 的 planName（"five_hour" / "seven_day"）走
 * `TIER_I18N_KEYS` 单独处理，这里只兜底中文 / 英文 / 日文等字面量。
 */
function labelForWindowName(name: string, t: (key: string, options?: Record<string, unknown>) => string): string {
  if (FIVE_HOUR_RE.test(name)) {
    return t("apiKeyStatusBar.fiveHourWindow", { defaultValue: name });
  }
  if (SEVEN_DAY_RE.test(name)) {
    return t("apiKeyStatusBar.sevenDayWindow", { defaultValue: name });
  }
  if (/30\s*d|30\s*天|月|month/i.test(name)) {
    return t("apiKeyStatusBar.thirtyDayWindow", { defaultValue: name });
  }
  if (/周|week/i.test(name)) {
    return t("apiKeyStatusBar.weeklyLimitWindow", { defaultValue: name });
  }
  return name;
}

/**
 * 毫秒 → 紧凑格式「Xs / Xm / Xh / Xd」。
 *
 * 与 provider 用量显示里 MiniMax 的「5小时:100%12m 7天:86%12m」
 * 风格保持一致——用户已经在「12m」里建立了「时间锚点」心智模型，
 * 这里复用同一个记号，避免「12:34」与「12m」两种风格来回切换造成认知负担。
 */
/**
 * 把毫秒数格式化为「紧凑剩余时长」——单单位版本，**输出字母必须
 * 是 ASCII 单字符 s/m/h/d**，因为结果会被 `parseCompactDurationToMs`
 * 反解回毫秒。
 *
 * 仅做英文 ASCII 输出。中文/日文等本地化版本走 `localizeCompactDuration`，
 * 仅用于 UI 展示，不能再喂给 parser。
 */
function formatRemaining(ms: number): string {
  const totalSec = Math.max(0, Math.ceil(ms / 1000));
  if (totalSec < 60) return `${totalSec}s`;
  const totalMin = Math.floor(totalSec / 60);
  if (totalMin < 60) return `${totalMin}m`;
  const totalHr = Math.floor(totalMin / 60);
  if (totalHr < 24) return `${totalHr}h`;
  const days = Math.floor(totalHr / 24);
  return `${days}d`;
}

/**
 * 把 formatDualCountdown / formatRemaining 的输出（例如 "3h0m" / "5d"）
 * 转换为本地化版本显示给用户。仅做字符串替换（[smhd] → 后端单位名），
 * 不重新算数字。
 */
function localizeCompactDuration(s: string, t: (key: string) => string): string {
  // 解析纯数字+单位段，逐段替换。避免一次性 replaceAll 因为 "3h0m" 应该
  // 变成 "3小时0分"（两个独立单位），不是 "3小时0小时" / "3小时0分"
  // 之类的连写替换——后者由 [smhd] 单位 token 自然分隔。
  const u = {
    s: t("apiKeyStatusBar.units.second"),
    m: t("apiKeyStatusBar.units.minute"),
    h: t("apiKeyStatusBar.units.hour"),
    d: t("apiKeyStatusBar.units.day"),
  };
  return s.replace(
    /(\d+)([smhd])/g,
    (_, n: string, unit: string) => `${n}${u[unit as "s" | "m" | "h" | "d"]}`,
  );
}

/**
 * 倒计时双单位格式——**内部 ASCII 版本**（与 `parseCompactDurationToMs`
 * 输出兼容）。仅做英文输出。
 *
 * - 5h 窗口：「XhYm」「Ym」「Xs」
 * - 7d 窗口：「XdYh」「XhYm」「Ym」「Xs」
 *
 * 负数（已过期）不渲染——调用方按 falsy 跳过。需要本地化展示时
 * 用 `localizeDualCountdown` 替换。
 */
function formatDualCountdown(ms: number, horizon: "5h" | "7d" | "30d"): string {
  if (!Number.isFinite(ms) || ms <= 0) return "";
  const totalSec = Math.floor(ms / 1000);
  const totalMin = Math.floor(totalSec / 60);
  const totalHr = Math.floor(totalMin / 60);
  const days = Math.floor(totalHr / 24);
  const hoursOfDay = totalHr % 24;
  const minutesOfHour = totalMin % 60;
  const secondsOfMinute = totalSec % 60;

  if (horizon === "5h") {
    if (totalHr >= 1) return `${totalHr}h${minutesOfHour}m`;
    if (totalMin >= 1) return `${totalMin}m`;
    return `${secondsOfMinute}s`;
  }
  if (days >= 1) return `${days}d${hoursOfDay}h`;
  if (totalHr >= 1) return `${totalHr}h${minutesOfHour}m`;
  if (totalMin >= 1) return `${totalMin}m`;
  return `${secondsOfMinute}s`;
}

/**
 * 紧凑字符串「12m / 2h / 3d / 30s」→ 毫秒。
 *
 * `parseUsageWindows` 拿到的「5小时:100%12m」里的 "12m" 段、`parseResetsAtForBar`
 * 拿到的 ISO 转换后字符串，都需要先回到 ms 才能喂给 formatDualCountdown。
 * 单字符单位（m / h / d / s）逐项匹配，避免误吃 "12month" 这种意外值。
 * 无法识别时返回 0——调用方按 falsy 跳过。
 */
function parseCompactDurationToMs(s: string): number {
  const m = s.trim().match(/^(\d+(?:\.\d+)?)\s*([smhd])$/i);
  if (!m) return 0;
  const n = parseFloat(m[1]);
  if (!Number.isFinite(n)) return 0;
  const unit = m[2].toLowerCase();
  switch (unit) {
    case "s":
      return n * 1000;
    case "m":
      return n * 60 * 1000;
    case "h":
      return n * 60 * 60 * 1000;
    case "d":
      return n * 24 * 60 * 60 * 1000;
    default:
      return 0;
  }
}

/** 解析 UsageData.extra 里的 `resetsAt`，返回紧凑格式（"12m" / "2h" / "3d"）。
 * 接受两种形态：
 *   - `"2026-07-01T17:00:00Z"`  → ISO 字符串
 *   - `'{"resetsAt":"2026-07-01T17:00:00Z",...}'` → JSON 包裹（ZenMux 等）
 * 解析失败 / 已过期 → 返回空串，调用方决定是否隐藏 reset 列。
 *
 * 返回值是 ASCII [smhd] 紧凑格式，便于 `parseCompactDurationToMs` 反解；
 * UI 展示时通过 `localizeCompactDuration` 转本地化版本。
 */
function parseResetsAtForBar(extra: string | undefined | null): string {
  if (!extra) return "";
  const trimmed = extra.trim();
  if (!trimmed) return "";
  let iso = trimmed;
  if (trimmed.startsWith("{")) {
    try {
      const obj = JSON.parse(trimmed);
      const v = obj?.resetsAt;
      if (typeof v !== "string") return "";
      iso = v;
    } catch {
      return "";
    }
  }
  const t1 = new Date(iso).getTime();
  if (!Number.isFinite(t1)) return "";
  const diff = t1 - Date.now();
  if (diff <= 0) return "";
  return formatRemaining(diff);
}

/**
 * 解析 UsageData.extra 里的「window:used%<reset>」段。
 *
 * 典型输入（MiniMax / PackyCode 等）：
 *   "5小时:100%12m 7天:86%12m"
 *   "5h:100%12m 7d:86%12m\n30d:50%3d"
 *   "{json}" → null（JSON 形式由 UsageFooter 单独处理，这里跳过）
 *
 * 规则：
 *   - 按空白 / 换行分段
 *   - 每段匹配 `<name>:<num>%<reset>`，三段都非空才算
 *   - 解析失败的段被丢弃；零有效段时返回空数组
 */
interface UsageWindow {
  name: string;
  used: number;
  reset: string;
}

function parseUsageWindows(extra: string | undefined | null): UsageWindow[] {
  if (!extra) return [];
  // JSON 形式由 UsageFooter 处理（QuotaTier），这里不要误吞。
  if (extra.trim().startsWith("{")) return [];
  const result: UsageWindow[] = [];
  for (const seg of extra.split(/[\s,;]+/)) {
    if (!seg) continue;
    const m = seg.match(/^(.+?):(\d+(?:\.\d+)?)%(\S+)$/);
    if (!m) continue;
    const name = m[1].trim();
    const used = parseFloat(m[2]);
    const reset = m[3].trim();
    if (!name || !reset || !Number.isFinite(used)) continue;
    result.push({ name, used, reset });
  }
  return result;
}
