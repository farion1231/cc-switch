export type TimeDomain = [number, number];

const MIN_VISIBLE_RATIO = 0.002;

export function normalizeDomain(domain: TimeDomain): TimeDomain {
  return domain[0] <= domain[1] ? domain : [domain[1], domain[0]];
}

export function clampDomain(
  domain: TimeDomain,
  bounds: TimeDomain,
  minimumSpan = 1,
): TimeDomain {
  const [boundStart, boundEnd] = normalizeDomain(bounds);
  const fullSpan = Math.max(boundEnd - boundStart, minimumSpan);
  const [rawStart, rawEnd] = normalizeDomain(domain);
  const requestedSpan = Math.min(
    Math.max(rawEnd - rawStart, minimumSpan),
    fullSpan,
  );
  let start = rawStart;
  let end = start + requestedSpan;
  if (start < boundStart) {
    start = boundStart;
    end = start + requestedSpan;
  }
  if (end > boundEnd) {
    end = boundEnd;
    start = end - requestedSpan;
  }
  return [Math.max(start, boundStart), Math.min(end, boundEnd)];
}

export function zoomDomain(
  domain: TimeDomain,
  bounds: TimeDomain,
  factor: number,
  anchor = 0.5,
  minimumSpan?: number,
): TimeDomain {
  const [start, end] = normalizeDomain(domain);
  const [boundStart, boundEnd] = normalizeDomain(bounds);
  const fullSpan = Math.max(boundEnd - boundStart, 1);
  const minSpan = minimumSpan ?? Math.max(fullSpan * MIN_VISIBLE_RATIO, 1);
  const span = Math.max(end - start, minSpan);
  const nextSpan = Math.min(Math.max(span * factor, minSpan), fullSpan);
  const safeAnchor = Math.min(Math.max(anchor, 0), 1);
  const anchorTime = start + span * safeAnchor;
  return clampDomain(
    [
      anchorTime - nextSpan * safeAnchor,
      anchorTime + nextSpan * (1 - safeAnchor),
    ],
    bounds,
    minSpan,
  );
}

export function panDomain(
  domain: TimeDomain,
  bounds: TimeDomain,
  fraction: number,
): TimeDomain {
  const [start, end] = normalizeDomain(domain);
  const delta = (end - start) * fraction;
  return clampDomain([start + delta, end + delta], bounds);
}

export function domainFromIndexes(
  values: readonly number[],
  startIndex: number,
  endIndex: number,
  bucketSeconds: readonly number[],
): TimeDomain | null {
  if (values.length === 0) return null;
  const start = Math.min(
    Math.max(Math.floor(startIndex), 0),
    values.length - 1,
  );
  const end = Math.min(
    Math.max(Math.floor(endIndex), start),
    values.length - 1,
  );
  const bucketMs = Math.max(bucketSeconds[end] ?? 0, 1) * 1000;
  return [values[start], values[end] + bucketMs];
}
