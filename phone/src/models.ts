import type { ModelInfo, Provider } from '../../src/lib/types';
import { request } from './link.svelte';

const cache = new Map<string, Promise<ModelInfo[]>>();

/** The AI's models for an account (asked from Wren once per page load). */
export function loadModels(provider: Provider, accountId: string): Promise<ModelInfo[]> {
  const key = `${provider}|${accountId}`;
  let p = cache.get(key);
  if (!p) {
    p = request<ModelInfo[]>({ op: 'models', provider, accountId }, 90_000).catch(() => {
      cache.delete(key);
      return [];
    });
    cache.set(key, p);
  }
  return p;
}

export const EFFORTS: Record<string, string> = {
  none: 'None',
  minimal: 'Minimal',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'Extra high',
  max: 'Max',
  ultra: 'Ultra',
};
