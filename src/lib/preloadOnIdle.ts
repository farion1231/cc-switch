/**
 * 页面空闲时预先加载一个懒加载模块（`import()`），让第一次真正用到时不用现等。
 *
 * 编辑 / 新增供应商页是懒加载的，连同表单要拉一百多个模块。`pnpm dev` 下 Vite 第一次请求
 * 时才逐个转译，重启后第一次点「编辑」要等一两秒；打包后也要现取、现编译这一大块。
 * 启动完成后趁空闲先拉好，点的时候就是现成的。失败不影响什么，用到时会再加载一次。
 */
export function preloadOnIdle(load: () => Promise<unknown>): () => void {
  const run = () => {
    load().catch(() => undefined);
  };
  if (typeof window.requestIdleCallback === "function") {
    const handle = window.requestIdleCallback(run, { timeout: 5000 });
    return () => window.cancelIdleCallback(handle);
  }
  const handle = window.setTimeout(run, 2000);
  return () => window.clearTimeout(handle);
}
