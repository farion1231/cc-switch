export type VerticalBounds = { top: number; bottom: number };

export function isWithinVerticalViewport(
  bounds: VerticalBounds,
  viewportHeight: number,
  tolerance = 1,
): boolean {
  return (
    bounds.top >= -tolerance && bounds.bottom <= viewportHeight + tolerance
  );
}
