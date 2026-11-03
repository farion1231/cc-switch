export interface Point {
  x: number;
  y: number;
}

export interface Box {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

/** 子面板靠主面板那条边上下各放宽一点：斜着往子面板的角上移也算 */
const TOLERANCE = 20;

/**
 * 鼠标从 `from` 移到 `to`，是不是正朝子面板去（和原生菜单一样的三角区判断）：
 * 到子面板近边上角的斜率在变小、到下角的斜率在变大，说明落点还在指向子面板的三角里。
 * 竖着在应用行之间移动不算，这时应立即切换。
 */
export function aimingAt(
  from: Point,
  to: Point,
  box: Box,
  side: "left" | "right",
): boolean {
  if (from.x === to.x && from.y === to.y) return false;
  const edgeX = side === "left" ? box.right : box.left;
  const ahead = (p: Point) => (side === "left" ? p.x - edgeX : edgeX - p.x);
  if (ahead(from) <= 0 || ahead(to) <= 0) return false;
  const slope = (p: Point, y: number) => (y - p.y) / ahead(p);
  const top = box.top - TOLERANCE;
  const bottom = box.bottom + TOLERANCE;
  return (
    slope(to, top) < slope(from, top) && slope(to, bottom) > slope(from, bottom)
  );
}
