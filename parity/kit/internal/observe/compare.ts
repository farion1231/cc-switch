// Text comparison for recordings. Exact equality passes; anything else is a difference, shown as
// the first differing region with context.

export function firstDifference(expected: string, actual: string, context = 3, maxLines = 40): string | null {
  if (expected === actual) return null;
  const a = expected.split("\n");
  const b = actual.split("\n");
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length - 1, endB = b.length - 1;
  while (endA >= start && endB >= start && a[endA] === b[endB]) { endA--; endB--; }
  const from = Math.max(0, start - context);
  const out: string[] = [`@@ line ${start + 1} @@`];
  for (let i = from; i < start; i++) out.push(`  ${a[i]}`);
  const removed = a.slice(start, endA + 1), added = b.slice(start, endB + 1);
  removed.slice(0, maxLines).forEach((l) => out.push(`- ${l}`));
  if (removed.length > maxLines) out.push(`- … ${removed.length - maxLines} more`);
  added.slice(0, maxLines).forEach((l) => out.push(`+ ${l}`));
  if (added.length > maxLines) out.push(`+ … ${added.length - maxLines} more`);
  for (let i = endA + 1; i < Math.min(a.length, endA + 1 + context); i++) out.push(`  ${a[i]}`);
  return out.join("\n");
}
