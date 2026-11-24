import { invoke } from '@tauri-apps/api/core';
import type { Conversation, ConversationMeta } from './types';

/** Saved conversations (stored only on this computer). */
export const history = $state<{ list: ConversationMeta[] }>({ list: [] });

export async function loadHistory() {
  history.list = await invoke<ConversationMeta[]>('list_conversations');
}

const timers = new Map<string, ReturnType<typeof setTimeout>>();

/** Save soon (debounced per conversation). */
export function saveConversation(conv: Conversation) {
  clearTimeout(timers.get(conv.id));
  timers.set(
    conv.id,
    setTimeout(async () => {
      timers.delete(conv.id);
      try {
        const meta = await invoke<ConversationMeta>('save_conversation', { conversation: conv });
        const i = history.list.findIndex((m) => m.id === meta.id);
        if (i >= 0) history.list.splice(i, 1);
        history.list.unshift(meta);
      } catch (e) {
        console.error('save failed', e);
      }
    }, 400),
  );
}

export async function getConversation(id: string) {
  return invoke<Conversation>('get_conversation', { id });
}

export async function deleteConversation(id: string) {
  await invoke('delete_conversation', { id });
  history.list = history.list.filter((m) => m.id !== id);
}

export async function renameConversation(id: string, title: string) {
  await invoke('rename_conversation', { id, title });
  const m = history.list.find((x) => x.id === id);
  if (m) m.title = title.trim();
}
