import { describe, it, expect } from "vitest";
import {
  resolveDisplayUsage,
  isTransientUsageError,
  KEEP_LAST_GOOD_MS,
  type LastGoodUsage,
} from "@/lib/query/queries";
import type { UsageResult } from "@/types";

// keep-last-good 的纯决策逻辑：仅"瞬时/网络类"失败才在 KEEP_LAST_GOOD_MS 窗口内继续
// 展示上一次成功；确定性失败（鉴权/空 key/未知供应商等）必须立即透出。

const ok = (remaining: number): UsageResult => ({
  success: true,
  data: [{ remaining, unit: "USD" }],
});
// 默认用网络类错误（瞬时），需要确定性失败时显式传入。
const fail = (error = "Network error: connection reset"): UsageResult => ({
  success: false,
  error,
});

const T0 = 1_000_000_000_000; // 任意基准时刻（ms）

describe("isTransientUsageError", () => {
  it("网络类失败 → 瞬时（true）", () => {
    expect(isTransientUsageError(fail("Network error: timed out"))).toBe(true);
    expect(isTransientUsageError(fail("Request failed: timed out"))).toBe(true);
    expect(isTransientUsageError(fail("请求失败: 连接超时"))).toBe(true);
    expect(isTransientUsageError(fail("Failed to read response: eof"))).toBe(
      true,
    );
    expect(isTransientUsageError(fail("读取响应失败: eof"))).toBe(true);
  });

  it("确定性失败 → 非瞬时（false），必须立即透出", () => {
    expect(
      isTransientUsageError(fail("Authentication failed (HTTP 401)")),
    ).toBe(false);
    expect(isTransientUsageError(fail("API key is empty"))).toBe(false);
    expect(isTransientUsageError(fail("Unknown balance provider"))).toBe(false);
    expect(isTransientUsageError(fail("Unknown coding plan provider"))).toBe(
      false,
    );
    expect(isTransientUsageError(fail("API error (HTTP 400): bad"))).toBe(
      false,
    );
    expect(isTransientUsageError(fail("Failed to parse response: x"))).toBe(
      false,
    );
  });

  it("HTTP 5xx / 429（限流）→ 瞬时（true）；其余 4xx → 非瞬时（false）", () => {
    expect(isTransientUsageError(fail("API error (HTTP 500): oops"))).toBe(
      true,
    );
    expect(
      isTransientUsageError(fail("HTTP 503 Service Unavailable : x")),
    ).toBe(true);
    expect(
      isTransientUsageError(fail("API error (HTTP 502): bad gateway")),
    ).toBe(true);
    // 429 是限流：稍后重试即可恢复，归瞬时——且不应清空 keep-last-good 快照
    expect(
      isTransientUsageError(fail("API error (HTTP 429): rate limited")),
    ).toBe(true);
    expect(
      isTransientUsageError(fail("HTTP 429 Too Many Requests : x")),
    ).toBe(true);
    expect(
      isTransientUsageError(fail("Authentication failed (HTTP 403)")),
    ).toBe(false);
    expect(
      isTransientUsageError(fail("Authentication failed (HTTP 401)")),
    ).toBe(false);
  });

  it("成功 / 无错误信息 → false", () => {
    expect(isTransientUsageError(ok(1))).toBe(false);
    expect(isTransientUsageError({ success: false })).toBe(false);
  });
});

describe("resolveDisplayUsage (keep-last-good)", () => {
  it("成功结果：原样展示并记录为 lastGood，lastQueriedAt=获取时刻", () => {
    const success = ok(42);
    const r = resolveDisplayUsage(success, T0, null, T0);
    expect(r.data).toBe(success);
    expect(r.lastQueriedAt).toBe(T0);
    expect(r.lastGood).toEqual({ data: success, at: T0 });
  });

  it("瞬时失败 + 窗口内有上次成功：继续展示成功值，lastQueriedAt 指向成功时刻", () => {
    const prev: LastGoodUsage = { data: ok(42), at: T0 };
    const now = T0 + KEEP_LAST_GOOD_MS - 1; // 刚好仍在窗口内
    const r = resolveDisplayUsage(fail(), now, prev, now);
    expect(r.data).toBe(prev.data); // 展示的是上次成功
    expect(r.lastQueriedAt).toBe(T0); // 时间戳反映成功的年龄
    expect(r.lastGood).toBe(prev); // 失败不更新 lastGood
  });

  it("瞬时失败 + 上次成功已过期（>= 窗口）：展示失败本身", () => {
    const prev: LastGoodUsage = { data: ok(42), at: T0 };
    const now = T0 + KEEP_LAST_GOOD_MS; // 边界：恰好到 10 分钟即过期
    const failure = fail();
    const r = resolveDisplayUsage(failure, now, prev, now);
    expect(r.data).toBe(failure);
    expect(r.lastQueriedAt).toBe(now);
    expect(r.lastGood).toBe(prev);
  });

  it("确定性失败（鉴权/空 key/未知供应商）：即使窗口内有上次成功也立即透出，并清空 lastGood", () => {
    const prev: LastGoodUsage = { data: ok(42), at: T0 };
    const now = T0 + 1000; // 远在窗口内
    for (const failure of [
      fail("Authentication failed (HTTP 401)"),
      fail("API key is empty"),
      fail("Unknown coding plan provider"),
    ]) {
      const r = resolveDisplayUsage(failure, now, prev, now);
      expect(r.data).toBe(failure); // 不掩盖 → 透出确定性失败
      expect(r.lastQueriedAt).toBe(now);
      expect(r.lastGood).toBeNull(); // 旧快照已不可信 → 清空，防止后续被复活
    }
  });

  it("确定性失败清空 lastGood：随后的网络抖动不会复活旧成功", () => {
    // 成功 → 记录 lastGood
    const afterSuccess = resolveDisplayUsage(ok(42), T0, null, T0);
    expect(afterSuccess.lastGood).not.toBeNull();
    // 401 鉴权失败 → 透出失败并清空 lastGood
    const afterAuthFail = resolveDisplayUsage(
      fail("Authentication failed (HTTP 401)"),
      T0 + 1000,
      afterSuccess.lastGood,
      T0 + 1000,
    );
    expect(afterAuthFail.lastGood).toBeNull();
    // 随后一次网络抖动（瞬时）→ lastGood 已空 → 不复活旧成功，照常透出失败
    const netFail = fail();
    const afterBlip = resolveDisplayUsage(
      netFail,
      T0 + 2000,
      afterAuthFail.lastGood,
      T0 + 2000,
    );
    expect(afterBlip.data).toBe(netFail);
    expect(afterBlip.lastGood).toBeNull();
  });

  it("瞬时失败 + 从无成功记录：展示失败本身", () => {
    const failure = fail();
    const now = T0 + 5000;
    const r = resolveDisplayUsage(failure, now, null, now);
    expect(r.data).toBe(failure);
    expect(r.lastQueriedAt).toBe(now);
    expect(r.lastGood).toBeNull();
  });

  it("新的成功覆盖旧的 lastGood", () => {
    const prev: LastGoodUsage = { data: ok(42), at: T0 };
    const fresh = ok(7);
    const now = T0 + 60_000;
    const r = resolveDisplayUsage(fresh, now, prev, now);
    expect(r.data).toBe(fresh);
    expect(r.lastGood).toEqual({ data: fresh, at: now });
  });

  it("加载中（raw=undefined）：data 为 undefined，lastGood 不变", () => {
    const prev: LastGoodUsage = { data: ok(42), at: T0 };
    const r = resolveDisplayUsage(undefined, 0, prev, T0 + 1000);
    expect(r.data).toBeUndefined();
    expect(r.lastQueriedAt).toBeNull();
    expect(r.lastGood).toBe(prev);
  });

  it("dataUpdatedAt 为 0 的成功：用注入的 now 作为获取时刻", () => {
    const success = ok(1);
    const now = T0 + 123;
    const r = resolveDisplayUsage(success, 0, null, now);
    expect(r.lastGood).toEqual({ data: success, at: now });
  });

  it("429 限流：窗口内继续展示上次成功，且不清空 lastGood", () => {
    const prev: LastGoodUsage = { data: ok(42), at: T0 };
    const now = T0 + 1000;
    const r = resolveDisplayUsage(
      fail("API error (HTTP 429): rate limited"),
      now,
      prev,
      now,
    );
    expect(r.data).toBe(prev.data); // 掩盖：继续展示上次成功
    expect(r.lastGood).toBe(prev); // 快照保留，抖动过去后可继续兜底
  });
});

// 订阅系 hooks（useSubscriptionQuota / useCodexOauthQuota）复用同一决策函数，
// 这里用 SubscriptionQuota 的形状锁定泛型行为。
describe("resolveDisplayUsage（SubscriptionQuota 形状）", () => {
  interface QuotaLike {
    success: boolean;
    error?: string | null;
    credentialStatus: string;
    queriedAt: number | null;
  }
  const quotaOk = (queriedAt: number): QuotaLike => ({
    success: true,
    error: null,
    credentialStatus: "valid",
    queriedAt,
  });
  const quotaFail = (error: string): QuotaLike => ({
    success: false,
    error,
    credentialStatus: "valid",
    queriedAt: null,
  });

  it("瞬时 5xx：窗口内继续展示上次成功的 quota（含其 queriedAt）", () => {
    const prev = { data: quotaOk(T0), at: T0 };
    const now = T0 + 1000;
    const r = resolveDisplayUsage(
      quotaFail("API error (HTTP 502): bad gateway"),
      now,
      prev,
      now,
    );
    expect(r.data).toBe(prev.data);
    expect(r.data?.queriedAt).toBe(T0); // 展示的相对时间指向旧成功
  });

  it("确定性失败（token 过期）：立即透出并清空 lastGood", () => {
    const prev = { data: quotaOk(T0), at: T0 };
    const now = T0 + 1000;
    const failure = quotaFail("OAuth token has expired");
    const r = resolveDisplayUsage(failure, now, prev, now);
    expect(r.data).toBe(failure);
    expect(r.lastGood).toBeNull();
  });
});

// invoke reject（后端 Err：网络/超时/读体中断）时 react-query 保留上次成功 data，
// `rejected` 标志让这份"陈旧成功"走同一 keep-last-good 窗口（锚定上次成功时刻
// dataUpdatedAt），而不是无限期被当作新鲜成功展示——否则「彻底断网」反而比
// 「单次 5xx」掩盖更久。
describe("resolveDisplayUsage（rejected：reject 保留的旧成功按窗口过期）", () => {
  it("窗口内：继续展示旧成功；prevLastGood=null（组件重挂丢 ref）也从 dataUpdatedAt 补种", () => {
    const stale = ok(42); // react-query 保留的旧成功，dataUpdatedAt = 其成功时刻
    const now = T0 + KEEP_LAST_GOOD_MS - 1;
    const r = resolveDisplayUsage(stale, T0, null, now, { rejected: true });
    expect(r.data).toBe(stale);
    expect(r.lastQueriedAt).toBe(T0); // 相对时间指向上次真实成功
    expect(r.lastGood).toEqual({ data: stale, at: T0 });
  });

  it("超窗（>= keepMs）：不再展示旧成功（data 置空由调用方合成失败占位），快照保留不清空", () => {
    const stale = ok(42);
    const now = T0 + KEEP_LAST_GOOD_MS;
    const r = resolveDisplayUsage(stale, T0, null, now, { rejected: true });
    expect(r.data).toBeUndefined();
    expect(r.lastQueriedAt).toBe(T0);
    expect(r.lastGood).toEqual({ data: stale, at: T0 }); // reject 属瞬时，不清快照
  });

  it("reject 且从无成功（raw=undefined，首次查询即失败）：data 为 undefined，lastGood 不变", () => {
    const r = resolveDisplayUsage(undefined, 0, null, T0, { rejected: true });
    expect(r.data).toBeUndefined();
    expect(r.lastQueriedAt).toBeNull();
    expect(r.lastGood).toBeNull();
  });

  it("reject 但保留的是确定性失败结果：照旧立即透出并清空 lastGood（rejected 只作用于成功值）", () => {
    const prev: LastGoodUsage = { data: ok(42), at: T0 };
    const failure = fail("Authentication failed (HTTP 401)");
    const r = resolveDisplayUsage(failure, T0 + 1000, prev, T0 + 1000, {
      rejected: true,
    });
    expect(r.data).toBe(failure);
    expect(r.lastGood).toBeNull();
  });

  it("rejected 未传（默认 false）：成功照常作为新鲜结果，既有语义不变", () => {
    const success = ok(7);
    const now = T0 + KEEP_LAST_GOOD_MS * 3; // 距 T0 很久也无妨：非 reject 的成功就是新鲜的
    const r = resolveDisplayUsage(success, now, null, now);
    expect(r.data).toBe(success);
    expect(r.lastGood).toEqual({ data: success, at: now });
  });
});
