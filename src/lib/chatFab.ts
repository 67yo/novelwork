export const CHAT_FAB_SIZE = 48;
export const CHAT_FAB_MARGIN = 16;
export const CHAT_FAB_EDGE = 8;

export type ChatFabPos = {
  right?: number;
  bottom?: number;
  x?: number;
  y?: number;
};

export function clampFabOffsets(
  p: ChatFabPos | null | undefined,
  vw: number,
  vh: number,
): { right: number; bottom: number } {
  let right: number;
  let bottom: number;
  if (
    p != null &&
    Number.isFinite(p.right) &&
    Number.isFinite(p.bottom)
  ) {
    right = Number(p.right);
    bottom = Number(p.bottom);
  } else {
    const x = Number(p?.x);
    const y = Number(p?.y);
    right = !Number.isFinite(x) || x < 0 ? CHAT_FAB_MARGIN : vw - x - CHAT_FAB_SIZE;
    bottom = !Number.isFinite(y) || y < 0 ? CHAT_FAB_MARGIN : vh - y - CHAT_FAB_SIZE;
  }
  const maxR = Math.max(CHAT_FAB_EDGE, vw - CHAT_FAB_SIZE - CHAT_FAB_EDGE);
  const maxB = Math.max(CHAT_FAB_EDGE, vh - CHAT_FAB_SIZE - CHAT_FAB_EDGE);
  return {
    right: Math.min(Math.max(CHAT_FAB_EDGE, right), maxR),
    bottom: Math.min(Math.max(CHAT_FAB_EDGE, bottom), maxB),
  };
}
