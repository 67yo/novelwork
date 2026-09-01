/** Socket outward normals. Path control points follow these so vertical spine ≠ left-right bezier. */
const OUT: Record<string, { x: number; y: number }> = {
  top: { x: 0, y: -1 },
  bottom: { x: 0, y: 1 },
  left: { x: -1, y: 0 },
  right: { x: 1, y: 0 },
  wv: { x: 0, y: -1 },
  sr: { x: 1, y: 0 },
  wp: { x: -1, y: 0 },
};

export function storyConnectionPath(
  start: { x: number; y: number },
  end: { x: number; y: number },
  sourceKey = "bottom",
  targetKey = "top",
): string {
  const d0 = OUT[sourceKey] ?? OUT.bottom;
  const d1 = OUT[targetKey] ?? OUT.top;
  const k = Math.max(32, Math.hypot(end.x - start.x, end.y - start.y) * 0.4);
  return `M ${start.x} ${start.y} C ${start.x + d0.x * k} ${start.y + d0.y * k}, ${end.x + d1.x * k} ${end.y + d1.y * k}, ${end.x} ${end.y}`;
}
