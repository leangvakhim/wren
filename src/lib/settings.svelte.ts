import type { Provider } from './types';

export type OnLimit = 'off' | 'wait' | 'switch';

export interface Settings {
  /** What a pane does by itself when its account runs out of usage. */
  onLimit: OnLimit;
  /** Order of AIs to try when switching. */
  fallbackOrder: Provider[];
  /** When switching finds no other AI, wait for the reset instead. */
  waitIfNoOther: boolean;
  /** System notifications while Wren is in the back (per computer). */
  notify: NotifySettings;
}

export interface NotifySettings {
  /** Main switch. */
  enabled: boolean;
  /** A pane waits for Allow / Deny (also "Plan ready" and Antigravity's refusals). */
  approval: boolean;
  /** A pane shows a question form. */
  question: boolean;
  /** A task finished. */
  done: boolean;
  /** A task stopped with an error or ran out of usage. */
  problems: boolean;
  sound: boolean;
}

const KEY = 'wren.settings.v1';
const DEFAULTS: Settings = {
  onLimit: 'switch',
  fallbackOrder: ['claude', 'codex', 'antigravity'],
  waitIfNoOther: true,
  notify: { enabled: true, approval: true, question: true, done: true, problems: true, sound: true },
};

function load(): Settings {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? 'null');
    if (s && typeof s === 'object') return { ...DEFAULTS, ...s, notify: { ...DEFAULTS.notify, ...s.notify } };
  } catch {}
  return { ...DEFAULTS, notify: { ...DEFAULTS.notify } };
}

/** App settings, saved on this computer only. */
export const settings = $state<Settings>(load());

export function saveSettings() {
  try {
    localStorage.setItem(KEY, JSON.stringify($state.snapshot(settings)));
  } catch {}
}
