/**
 * 会话页耗时打点：一次操作从开始到各阶段各用了多久，结束时写到控制台。
 * 开发环境默认打开；正式版在控制台执行
 * `localStorage.setItem("cc-switch:session-perf", "1")` 打开。
 * 关闭时每个调用都只是一次 Map 查找，不影响页面。
 */

const FLAG = "cc-switch:session-perf";

interface SessionTrace {
  startedAt: number;
  stages: string[];
}

const traces = new Map<string, SessionTrace>();

const isEnabled = () => {
  if (import.meta.env.DEV && import.meta.env.MODE !== "test") return true;
  try {
    return window.localStorage.getItem(FLAG) === "1";
  } catch {
    return false;
  }
};

const elapsed = (trace: SessionTrace) =>
  Math.round(performance.now() - trace.startedAt);

/** 开始一次打点；同 id 的上一次没结束就直接作废 */
export const startSessionTrace = (id: string) => {
  if (!isEnabled()) return;
  traces.set(id, { startedAt: performance.now(), stages: [] });
};

/** 记一个阶段；同名阶段只记第一次 */
export const markSessionTrace = (id: string, stage: string) => {
  const trace = traces.get(id);
  if (!trace || trace.stages.some((item) => item.startsWith(`${stage}=`))) {
    return;
  }
  trace.stages.push(`${stage}=${elapsed(trace)}ms`);
};

/** 结束打点并输出 */
export const endSessionTrace = (id: string, stage = "done") => {
  const trace = traces.get(id);
  if (!trace) return;
  traces.delete(id);
  trace.stages.push(`${stage}=${elapsed(trace)}ms`);
  console.debug(`[sessions] ${id}: ${trace.stages.join(" ")}`);
};
