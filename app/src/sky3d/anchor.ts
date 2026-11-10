// The info panel (#299) sits beside the object it describes and always fully inside the view:
// to the right if it fits, else to the left, else above or below; then clamped to the edges.

export type Side = "right" | "left" | "below" | "above";

export function anchorPanel(
  target: { x: number; y: number; r: number },
  panel: { width: number; height: number },
  view: { width: number; height: number },
  gap = 14,
  margin = 12,
): { left: number; top: number; side: Side } {
  const maxLeft = view.width - margin - panel.width;
  const maxTop = view.height - margin - panel.height;
  const midTop = clamp(target.y - panel.height / 2, margin, maxTop);
  const right = target.x + target.r + gap;
  if (right <= maxLeft) return { left: right, top: midTop, side: "right" };
  const left = target.x - target.r - gap - panel.width;
  if (left >= margin) return { left, top: midTop, side: "left" };
  const midLeft = clamp(target.x - panel.width / 2, margin, maxLeft);
  const below = target.y + target.r + gap;
  if (below <= maxTop) return { left: midLeft, top: below, side: "below" };
  const above = target.y - target.r - gap - panel.height;
  if (above >= margin) return { left: midLeft, top: above, side: "above" };
  // Nothing fits cleanly (a tiny window): keep it inside the view, over the object if it must.
  return { left: midLeft, top: clamp(above, margin, maxTop), side: "above" };
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), Math.max(lo, hi));
}
