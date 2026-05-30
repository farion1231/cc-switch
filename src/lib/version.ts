/**
 * 轻量 semver 比较 —— 用于工具版本"是否有可用更新"的判断。
 *
 * 为什么不直接用字符串 `!==` 比较:版本号是偏序关系,"不相等"不等于"落后"。
 * 当本地装的是抢先/预发布通道(如 Claude Code 的 npm `next` tag,版本号反而
 * 高于 `latest`)时,字符串 `!==` 会把"领先"误判成"需要更新",造成永久误报
 * (永远提示可更新,点了升级又触发"版本未变"警告,提示消不掉)。
 */

interface ParsedVersion {
  /** 主版本三段 [major, minor, patch] */
  core: [number, number, number];
  /** 预发布标识符段,如 "2.1.156-beta.1" → ["beta", "1"];正式版为空数组 */
  pre: string[];
}

/** 解析 "2.1.156" / "2.1.156-beta.1";无法解析返回 null。 */
function parseVersion(v: string): ParsedVersion | null {
  const m = v.trim().match(/^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?/);
  if (!m) return null;
  return {
    core: [Number(m[1]), Number(m[2]), Number(m[3])],
    pre: m[4] ? m[4].split(".") : [],
  };
}

/**
 * 比较预发布段(遵循 semver):
 * - 双方都无预发布 → 相等
 * - 有预发布的版本 < 没有的(正式版更高)
 * - 都有则逐段比较:数字段按数值、数字段 < 非数字段、非数字段按 ASCII,
 *   前缀相同则段更多的更大
 */
function comparePre(a: string[], b: string[]): number {
  if (a.length === 0 && b.length === 0) return 0;
  if (a.length === 0) return 1;
  if (b.length === 0) return -1;
  const len = Math.min(a.length, b.length);
  for (let i = 0; i < len; i++) {
    const ai = a[i];
    const bi = b[i];
    const aNum = /^\d+$/.test(ai);
    const bNum = /^\d+$/.test(bi);
    if (aNum && bNum) {
      const d = Number(ai) - Number(bi);
      if (d !== 0) return d < 0 ? -1 : 1;
    } else if (aNum) {
      return -1;
    } else if (bNum) {
      return 1;
    } else if (ai !== bi) {
      return ai < bi ? -1 : 1;
    }
  }
  if (a.length === b.length) return 0;
  return a.length < b.length ? -1 : 1;
}

/**
 * 比较两个版本号。
 * @returns >0 表示 a 比 b 新;<0 表示 a 比 b 旧;0 表示相等或无法判定。
 *   任一无法解析时返回 0(保守:视为"无法判定差异",不触发更新提示)。
 */
export function compareVersions(a: string, b: string): number {
  const pa = parseVersion(a);
  const pb = parseVersion(b);
  if (!pa || !pb) return 0;
  for (let i = 0; i < 3; i++) {
    const d = pa.core[i] - pb.core[i];
    if (d !== 0) return d < 0 ? -1 : 1;
  }
  return comparePre(pa.pre, pb.pre);
}

/**
 * 是否有可用更新:仅当 `latest` 严格高于 `current` 时为 true。
 * 本地版本 ≥ latest(含抢先/预发布版反超 latest 的情况)一律返回 false。
 */
export function isUpdateAvailable(
  current: string | null | undefined,
  latest: string | null | undefined,
): boolean {
  if (!current || !latest) return false;
  return compareVersions(latest, current) > 0;
}
