import type { Provider } from './types';

export type OnLimit = 'off' | 'wait' | 'switch';

export interface Settings {
  /** What a pane does by itself when its account runs out of usage. */
  onLimit: OnLimit;
  /** Order of AIs to try when switching. */
  fallbackOrder: Provider[];
  /** When switching finds no other AI, wait for the reset instead. */
  waitIfNoOther: boolean;
}

const KEY = 'wren.settings.v1';
const DEFAULTS: Settings = {
  onLimit: 'switch',
  fallbackOrder: ['claude', 'codex', 'antigravity'],
  waitIfNoOther: true,
};

function load(): Settings {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? 'null');
    if (s && typeof s === 'object') return { ...DEFAULTS, ...s };
  } catch {}
  return { ...DEFAULTS };
}

/** App settings, saved on this computer only. */
export const settings = $state<Settings>(load());

export function saveSettings() {
  try {
    localStorage.setItem(KEY, JSON.stringify($state.snapshot(settings)));
  } catch {}
}
