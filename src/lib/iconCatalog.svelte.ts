import { invoke } from '@tauri-apps/api/core';
import { FOLDER_ICONS } from './folderIcons';

/**
 * Every icon a folder can have. Ids: Wren's own ("code"), "lucide:<name>"
 * (ships with Wren, loaded when first needed), "tabler:<name>" (downloaded by
 * you, see src-tauri/src/icons.rs), "emoji:<text>" and "own:<id>" (your pictures).
 */
export type Glyph = { kind: 'svg'; body: string } | { kind: 'emoji'; text: string } | { kind: 'img'; url: string };
export type CatalogIcon = { id: string; name: string; categories: string[]; search: string; body: string };
export type EmojiGroup = { name: string; items: { text: string; name: string }[] };
export type OwnIcon = { id: string; url: string };

type Pack = { version: string; icons: { id: string; category: string; tags: string; body: string }[] };

/** Lucide's and Tabler's category names, merged into one list. */
const TITLES: Record<string, string> = {
  account: 'People', people: 'People', gender: 'People', gestures: 'People',
  ai: 'AI', brand: 'Brands', computers: 'Devices', 'version control': 'Development', database: 'Development',
  emoji: 'Faces', mood: 'Faces', files: 'Files', document: 'Files', extensions: 'Files',
  finance: 'Money', currencies: 'Money', 'food-beverage': 'Food', gaming: 'Games', games: 'Games',
  'letters-digits': 'Letters & numbers', letters: 'Letters & numbers', numbers: 'Letters & numbers',
  medical: 'Health', health: 'Health', multimedia: 'Media', media: 'Media', navigation: 'Maps', map: 'Maps',
  'e-commerce': 'Shopping', sport: 'Sports', transportation: 'Transport', vehicles: 'Transport', logistics: 'Transport',
  sustainability: 'Nature', laundry: 'Home', electrical: 'Devices', zodiac: 'Symbols', logic: 'Math', badges: 'Symbols',
};
const title = (raw: string) => {
  const k = raw.trim().toLowerCase();
  if (!k) return 'Other';
  return TITLES[k] ?? k.charAt(0).toUpperCase() + k.slice(1).replace(/-/g, ' ');
};
const pretty = (name: string) => (name.charAt(0).toUpperCase() + name.slice(1)).replace(/-/g, ' ');

function toCatalog(prefix: string, icons: { name: string; cats: string[]; tags: string; body: string }[]): CatalogIcon[] {
  return icons.map((i) => ({
    id: `${prefix}:${i.name}`,
    name: pretty(i.name),
    categories: [...new Set(i.cats.length ? i.cats.map(title) : ['Other'])],
    search: `${i.name.replace(/-/g, ' ')} ${i.tags.replace(/,/g, ' ')}`.toLowerCase(),
    body: i.body,
  }));
}

class Catalog {
  /** null = not loaded yet. */
  lucide = $state.raw<CatalogIcon[] | null>(null);
  /** undefined = not checked yet, null = not downloaded. */
  tabler = $state.raw<CatalogIcon[] | null | undefined>(undefined);
  tablerVersion = $state('');
  emoji = $state.raw<EmojiGroup[] | null>(null);
  own = $state.raw<OwnIcon[] | null>(null);
  downloading = $state(false);
  error = $state('');
}
export const catalog = new Catalog();

export const BUILT_IN: CatalogIcon[] = FOLDER_ICONS.map((i) => ({ id: i.id, name: i.label, categories: [], search: `${i.id} ${i.label}`.toLowerCase(), body: i.body }));
const builtIn = new Map(BUILT_IN.map((i) => [i.id, i]));
let lucideById = new Map<string, CatalogIcon>();
let tablerById = new Map<string, CatalogIcon>();
const loading: Record<string, Promise<unknown> | undefined> = {};
const once = <T>(key: string, f: () => Promise<T>) => (loading[key] ??= f().finally(() => (loading[key] = undefined)));

export function loadLucide() {
  if (catalog.lucide) return Promise.resolve();
  return once('lucide', async () => {
    const data = (await import('./icons/lucide.json')).default as unknown as { version: string; icons: [string, string, string, string][] };
    const list = toCatalog('lucide', data.icons.map(([name, body, cats, tags]) => ({ name, body, cats: cats ? cats.split(' ') : [], tags })));
    lucideById = new Map(list.map((i) => [i.id, i]));
    catalog.lucide = list;
  });
}

function setTabler(pack: Pack | null) {
  const list = pack ? toCatalog('tabler', pack.icons.map((i) => ({ name: i.id, body: i.body, cats: i.category ? [i.category] : [], tags: i.tags }))) : null;
  tablerById = new Map((list ?? []).map((i) => [i.id, i]));
  catalog.tablerVersion = pack?.version ?? '';
  catalog.tabler = list;
}

export function loadTabler() {
  if (catalog.tabler !== undefined) return Promise.resolve();
  return once('tabler', async () => setTabler(await invoke<Pack | null>('icon_pack').catch(() => null)));
}

export async function downloadTabler() {
  catalog.downloading = true;
  catalog.error = '';
  try {
    setTabler(await invoke<Pack>('icon_pack_download'));
  } catch (e) {
    catalog.error = String(e);
  } finally {
    catalog.downloading = false;
  }
}

export async function removeTabler() {
  catalog.error = '';
  try {
    await invoke('icon_pack_remove');
    setTabler(null);
  } catch (e) {
    catalog.error = String(e);
  }
}

export function loadEmoji() {
  if (catalog.emoji) return Promise.resolve();
  return once('emoji', async () => {
    const data = (await import('./icons/emoji.json')).default as unknown as { groups: [string, [string, string][]][] };
    catalog.emoji = data.groups.map(([name, items]) => ({ name, items: items.map(([text, n]) => ({ text, name: n })) }));
  });
}

export function loadOwn() {
  if (catalog.own) return Promise.resolve();
  return once('own', async () => (catalog.own = await invoke<OwnIcon[]>('own_icons').catch(() => [])));
}

/** Copies pictures into Wren's folder; returns the last one added. */
export async function addOwn(paths: string[]): Promise<OwnIcon | null> {
  catalog.error = '';
  await loadOwn();
  let last: OwnIcon | null = null;
  for (const path of paths) {
    try {
      last = await invoke<OwnIcon>('add_own_icon', { path });
      catalog.own = [...(catalog.own ?? []), last];
    } catch (e) {
      catalog.error = `${path.split(/[\\/]/).pop()}: ${e}`;
    }
  }
  return last;
}

export async function removeOwn(id: string) {
  catalog.error = '';
  try {
    await invoke('remove_own_icon', { id });
    catalog.own = (catalog.own ?? []).filter((i) => i.id !== id);
  } catch (e) {
    catalog.error = String(e);
  }
}

/** Emoji ids are short text, shown as text (never as markup). */
export const emojiId = (text: string) => `emoji:${text}`;
export const isEmoji = (s: string) => s.length > 0 && s.length <= 16 && /\p{Extended_Pictographic}|\p{Regional_Indicator}|\u20E3/u.test(s) && !/[\p{L}\s<>&]/u.test(s.replace(/\p{Extended_Pictographic}/gu, ''));

/** What to draw for an icon id; null while its set is loading or when it's gone (show the plain folder). */
export function glyph(id: string): Glyph | null {
  const [kind, rest] = id.includes(':') ? [id.slice(0, id.indexOf(':')), id.slice(id.indexOf(':') + 1)] : ['', id];
  switch (kind) {
    case '':
      return { kind: 'svg', body: (builtIn.get(id) ?? BUILT_IN[0]).body };
    case 'emoji':
      return isEmoji(rest) ? { kind: 'emoji', text: rest } : null;
    case 'lucide':
      if (catalog.lucide) return svg(lucideById.get(id));
      loadLucide();
      return null;
    case 'tabler':
      if (catalog.tabler !== undefined) return svg(tablerById.get(id));
      loadTabler();
      return null;
    case 'own': {
      if (!catalog.own) {
        loadOwn();
        return null;
      }
      const own = catalog.own.find((i) => i.id === rest);
      return own ? { kind: 'img', url: own.url } : null;
    }
  }
  return null;
}
const svg = (i: CatalogIcon | undefined): Glyph | null => (i ? { kind: 'svg', body: i.body } : null);
