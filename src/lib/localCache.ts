/**
 * 页面数据的本地缓存（localStorage）：下次打开先显示上次的结果，后台再刷新。
 * 只是加速首屏的「旧数据」，读写失败（隐私模式、配额满、数据损坏）一律当作没有缓存。
 */
const PREFIX = "cc-switch-cache:";

export function readLocalCache<T>(key: string): T | undefined {
  try {
    const raw = localStorage.getItem(PREFIX + key);
    return raw ? (JSON.parse(raw) as T) : undefined;
  } catch {
    return undefined;
  }
}

export function writeLocalCache(key: string, value: unknown): void {
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify(value));
  } catch {
    // 写不进去就算了，下次照常从头加载
  }
}
