/**
 * 懒加载视图的 Suspense 占位。
 * 信号灯式三点脉冲，与设计语言一致。
 */
export function ViewFallback() {
  return (
    <div className="flex h-full min-h-[240px] w-full items-center justify-center">
      <div className="flex items-center gap-1.5" role="status" aria-label="Loading">
        {[0, 1, 2].map((i) => (
          <span
            key={i}
            className="h-1.5 w-1.5 animate-pulse rounded-full bg-primary/70"
            style={{
              animationDelay: `${i * 160}ms`,
              animationDuration: "900ms",
            }}
          />
        ))}
      </div>
    </div>
  );
}
