/** Workspace → GlobalChatPanel：投递一条用户消息（Chat 未挂载时先排队）。 */

type Listener = (text: string) => void;

const listeners = new Set<Listener>();
let pending: string | null = null;

export function enqueueGlobalChatSend(text: string) {
  const t = text.trim();
  if (!t) return;
  if (listeners.size === 0) {
    pending = t;
    return;
  }
  for (const fn of listeners) fn(t);
}

export function subscribeGlobalChatSend(fn: Listener): () => void {
  listeners.add(fn);
  if (pending) {
    const p = pending;
    pending = null;
    fn(p);
  }
  return () => {
    listeners.delete(fn);
  };
}
