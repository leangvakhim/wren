import { invoke } from '@tauri-apps/api/core';
import type { ModelInfo, Provider } from './types';

/** Model lists per AI+account, asked from the CLI once per run. */
export const modelLists = $state<Record<string, ModelInfo[]>>({});
const pending = new Map<string, Promise<void>>();

export function modelsKey(provider: Provider, accountId: string) {
  return `${provider}|${accountId}`;
}

export function ensureModels(provider: Provider, accountId: string) {
  const key = modelsKey(provider, accountId);
  if (modelLists[key] || pending.has(key)) return;
  const p = invoke<ModelInfo[]>('list_models', { provider, accountId })
    .then((list) => {
      modelLists[key] = list;
    })
    .catch(() => {
      modelLists[key] = [];
    })
    .finally(() => pending.delete(key));
  pending.set(key, p);
}

export const EFFORT_LABEL: Record<string, string> = {
  none: 'None',
  minimal: 'Minimal',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'Extra high',
  max: 'Max',
  ultra: 'Ultra',
};

export const effortLabel = (e: string) => EFFORT_LABEL[e] ?? e;

/** The full name for a model id the CLI reported (e.g. "claude-opus-5-5" -> "Opus 5.5"). */
export function modelName(list: ModelInfo[] | undefined, id: string | null | undefined): string {
  if (!id) return '';
  const m = list?.find((x) => x.id === id || x.resolved === id || x.label === id);
  return m ? m.label : id;
}
