import { useEffect, useState } from "react";

/**
 * flag 变 true 后要持续 delay 毫秒才返回 true；变 false 立即返回 false。
 * 用在「写入期间整页禁用」上：一次很快的写入（点一个勾选格）不让整页按钮闪一下变灰，
 * 写得久了才显示禁用态。真正的拦截仍然看原始 flag / 写锁，这里只管外观。
 */
export function useDelayedFlag(flag: boolean, delay = 300): boolean {
  const [delayed, setDelayed] = useState(false);

  useEffect(() => {
    if (!flag) {
      setDelayed(false);
      return;
    }
    const timer = window.setTimeout(() => setDelayed(true), delay);
    return () => window.clearTimeout(timer);
  }, [flag, delay]);

  return flag && delayed;
}
