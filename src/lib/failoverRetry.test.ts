import {
  DEFAULT_FAILOVER_RETRY_POLICY,
  normalizeFailoverRetryPolicy,
} from "@/lib/failoverRetry";

describe("normalizeFailoverRetryPolicy", () => {
  it("uses zero as the default retry count", () => {
    expect(DEFAULT_FAILOVER_RETRY_POLICY.maxRetries).toBe(0);
    expect(normalizeFailoverRetryPolicy().maxRetries).toBe(0);
  });

  it("preserves an explicit finite zero retry value", () => {
    expect(
      normalizeFailoverRetryPolicy({
        mode: "finite",
        maxRetries: 0,
        baseDelaySeconds: 3,
        maxDelaySeconds: 30,
        backoffMultiplier: 2,
      }).maxRetries,
    ).toBe(0);
  });

  it("clamps negative retry values up to zero", () => {
    expect(
      normalizeFailoverRetryPolicy({
        mode: "finite",
        maxRetries: -1,
      }).maxRetries,
    ).toBe(0);
  });
});
