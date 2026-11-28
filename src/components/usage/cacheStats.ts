/** 输入分项已经由后端按各客户端语义归一化；缺失与显式零必须区分。 */
export interface CacheTokenFields {
  freshInputTokens?: number;
  cacheCreationTokens?: number;
  cacheReadTokens?: number;
}

export function getCacheStats(stat: CacheTokenFields) {
  const {
    freshInputTokens: fresh,
    cacheCreationTokens: write,
    cacheReadTokens: read,
  } = stat;
  if (
    fresh == null ||
    write == null ||
    read == null ||
    ![fresh, write, read].every((value) => Number.isFinite(value) && value >= 0)
  )
    return null;
  const input = fresh + write + read;
  if (!Number.isFinite(input)) return null;
  return { fresh, write, read, input, rate: input > 0 ? read / input : null };
}

export function cacheHitLabel(stat: CacheTokenFields): string {
  const rate = getCacheStats(stat)?.rate;
  return rate == null ? "—" : `${(rate * 100).toFixed(1)}%`;
}
