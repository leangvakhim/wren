import { invoke } from '@tauri-apps/api/core';

/** The icon you chose for a folder ('' color = the text color). */
export type FolderLook = { icon: string; color: string };

const KEY = 'wren.folders.v1';

function load(): Record<string, FolderLook> {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    return v && typeof v === 'object' ? v : {};
  } catch {
    return {};
  }
}

/** Your choices per folder path (this computer only), and what Wren suggests from the files. */
export const folders = $state<{ chosen: Record<string, FolderLook>; auto: Record<string, string | null> }>({ chosen: load(), auto: {} });

const asking = new Set<string>();

/** The icon to show for a folder: your choice, else Wren's suggestion, else the plain folder. */
export function lookFor(cwd: string): FolderLook & { auto: boolean } {
  const own = folders.chosen[cwd];
  if (own) return { ...own, auto: false };
  if (cwd && !(cwd in folders.auto) && !asking.has(cwd)) {
    asking.add(cwd);
    invoke<string | null>('folder_kind', { path: cwd })
      .then((k) => (folders.auto[cwd] = k))
      .catch(() => (folders.auto[cwd] = null))
      .finally(() => asking.delete(cwd));
  }
  return { icon: folders.auto[cwd] ?? 'folder', color: '', auto: true };
}

/** null = back to Wren's suggestion. */
export function setLook(cwd: string, look: FolderLook | null) {
  if (look) folders.chosen[cwd] = look;
  else delete folders.chosen[cwd];
  try {
    localStorage.setItem(KEY, JSON.stringify(folders.chosen));
  } catch {}
}
