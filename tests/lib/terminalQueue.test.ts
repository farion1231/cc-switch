import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { terminalQueue } from "@/lib/terminalQueue";

/**
 * 排队发送的核心契约：
 * 1. 入队后自动按顺序派发，一次只发一条；
 * 2. 上一条「执行完成」（输出静默达到 idleMs）后才发下一条；
 * 3. 队列按终端实例隔离。
 */
describe("terminalQueue", () => {
  beforeEach(() => {
    window.localStorage.clear();
    terminalQueue.updateSettings({
      idleMs: 1000,
      startTimeoutMs: 2000,
      queueMode: true,
    });
    vi.useFakeTimers();
  });

  afterEach(() => {
    for (const id of ["t1", "t2"]) {
      terminalQueue.clear(id);
      terminalQueue.dispose(id);
    }
    vi.useRealTimers();
  });

  it("按顺序派发：上一条静默完成后才发下一条", async () => {
    const sent: string[] = [];
    terminalQueue.registerSender("t1", async (text) => {
      sent.push(text);
    });
    terminalQueue.enqueue("t1", "第一条");
    terminalQueue.enqueue("t1", "第二条");
    terminalQueue.start("t1");

    // 首次派发
    await vi.advanceTimersByTimeAsync(400);
    expect(sent).toEqual(["第一条"]);

    // 终端持续有输出 → 不能提前发下一条（间隔小于 idleMs）
    terminalQueue.notifyOutput("t1");
    await vi.advanceTimersByTimeAsync(500);
    expect(sent).toEqual(["第一条"]);
    terminalQueue.notifyOutput("t1");
    await vi.advanceTimersByTimeAsync(500);
    expect(sent).toEqual(["第一条"]);

    // 输出静止超过 idleMs → 判定上一条完成，派发下一条
    await vi.advanceTimersByTimeAsync(2000);
    expect(sent).toEqual(["第一条", "第二条"]);

    // 队列跑完后自动停止
    await vi.advanceTimersByTimeAsync(4000);
    expect(terminalQueue.get("t1").running).toBe(false);
  });

  it("会话未就绪时挂起，注册发送器后自动补发", async () => {
    const sent: string[] = [];
    terminalQueue.enqueue("t1", "待会话就绪");
    terminalQueue.start("t1");
    await vi.advanceTimersByTimeAsync(1000);
    expect(sent).toEqual([]);
    expect(terminalQueue.get("t1").running).toBe(true);

    terminalQueue.registerSender("t1", async (text) => {
      sent.push(text);
    });
    await vi.advanceTimersByTimeAsync(400);
    expect(sent).toEqual(["待会话就绪"]);
  });

  it("暂停后不再派发，继续后接着发", async () => {
    const sent: string[] = [];
    terminalQueue.registerSender("t1", async (text) => {
      sent.push(text);
    });
    terminalQueue.enqueue("t1", "A");
    terminalQueue.enqueue("t1", "B");
    terminalQueue.start("t1");
    await vi.advanceTimersByTimeAsync(400);
    expect(sent).toEqual(["A"]);

    terminalQueue.pause("t1");
    await vi.advanceTimersByTimeAsync(5000);
    expect(sent).toEqual(["A"]);

    terminalQueue.start("t1");
    await vi.advanceTimersByTimeAsync(5000);
    expect(sent).toEqual(["A", "B"]);
  });

  it("跳过后立即派发下一条", async () => {
    const sent: string[] = [];
    terminalQueue.registerSender("t1", async (text) => {
      sent.push(text);
    });
    terminalQueue.enqueue("t1", "A");
    terminalQueue.enqueue("t1", "B");
    terminalQueue.start("t1");
    await vi.advanceTimersByTimeAsync(400);
    expect(sent).toEqual(["A"]);

    terminalQueue.skip("t1");
    await vi.advanceTimersByTimeAsync(400);
    expect(sent).toEqual(["A", "B"]);
  });

  it("队列按终端实例隔离，互不干扰", async () => {
    const sentA: string[] = [];
    const sentB: string[] = [];
    terminalQueue.registerSender("t1", async (text) => {
      sentA.push(text);
    });
    terminalQueue.registerSender("t2", async (text) => {
      sentB.push(text);
    });

    terminalQueue.enqueue("t1", "A1");
    terminalQueue.start("t1");
    await vi.advanceTimersByTimeAsync(400);
    expect(sentA).toEqual(["A1"]);
    expect(sentB).toEqual([]);

    // t2 独立计数；清空 t2 不影响 t1（t1 的 A1 已发出，处于 sending）
    expect(terminalQueue.get("t1").items[0].status).toBe("sending");
    terminalQueue.enqueue("t2", "B1");
    expect(terminalQueue.pendingCount("t2")).toBe(1);

    terminalQueue.clear("t2");
    expect(terminalQueue.pendingCount("t2")).toBe(0);
  });

  it("发送失败时标记失败并继续下一条", async () => {
    const sent: string[] = [];
    let first = true;
    terminalQueue.registerSender("t1", async (text) => {
      sent.push(text);
      if (first) {
        first = false;
        throw new Error("会话不可用");
      }
    });
    terminalQueue.enqueue("t1", "会失败");
    terminalQueue.enqueue("t1", "正常");
    terminalQueue.start("t1");
    await vi.advanceTimersByTimeAsync(400);
    expect(sent).toEqual(["会失败"]);
    expect(terminalQueue.get("t1").items[0].status).toBe("error");

    await vi.advanceTimersByTimeAsync(4000);
    expect(sent).toEqual(["会失败", "正常"]);
  });
});
