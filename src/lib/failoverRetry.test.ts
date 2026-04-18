import {
  DEFAULT_FAILOVER_RETRY_POLICY,
  normalizeFailoverRetryPolicy,
} from "@/lib/failoverRetry";

describe("normalizeFailoverRetryPolicy", () => {
  it("uses zero as the default retry count", () => {
    expect(DEFAULT_FAILOVER_RETRY_POLICY.maxRetries).toBe(0);
    expect(normalizeFailoverRetryPolicy().maxRetries).toBe(0);
  });

  it("uses the default non-retryable keywords", () => {
    expect(DEFAULT_FAILOVER_RETRY_POLICY.nonRetryableKeywords).toEqual([
      "invalid_api_key",
      "invalid_request",
      "context_length_exceeded",
    ]);
    expect(normalizeFailoverRetryPolicy().nonRetryableKeywords).toEqual([
      "invalid_api_key",
      "invalid_request",
      "context_length_exceeded",
    ]);
  });

  it("preserves an explicit finite zero retry value", () => {
    expect(
      normalizeFailoverRetryPolicy({
        mode: "finite",
        maxRetries: 0,
        baseDelaySeconds: 3,
        maxDelaySeconds: 30,
        backoffMultiplier: 2,
        nonRetryableKeywords: [],
      }).maxRetries,
    ).toBe(0);
  });

  it("clamps negative retry values up to zero", () => {
    expect(
      normalizeFailoverRetryPolicy({
        mode: "finite",
        maxRetries: -1,
        nonRetryableKeywords: [],
      }).maxRetries,
    ).toBe(0);
  });

  it("preserves an explicit empty non-retryable keyword list", () => {
    expect(
      normalizeFailoverRetryPolicy({
        mode: "finite",
        maxRetries: 1,
        nonRetryableKeywords: [],
      }).nonRetryableKeywords,
    ).toEqual([]);
  });

  it("trims and deduplicates keyword values by normalized form", () => {
    expect(
      normalizeFailoverRetryPolicy({
        mode: "finite",
        maxRetries: 1,
        nonRetryableKeywords: [
          " invalid_api_key ",
          "Invalid API Key",
          "context-length-exceeded",
        ],
      }).nonRetryableKeywords,
    ).toEqual(["invalid_api_key", "context-length-exceeded"]);
  });
});
