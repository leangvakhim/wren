import { invoke } from '@tauri-apps/api/core';
import type { AccountLimits, PlanLimits, Provider } from './types';

/**
 * Plan limits (5-hour / weekly windows, daily tokens) per account, as each
 * AI's own CLI reports them. Kept on this computer only (localStorage) so the
 * last known numbers show right away; refreshed from the CLI on demand.
 */
const KEY = 'wren.limits.v1';

function load(): Record<string, AccountLimits> {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null');
    if (v && typeof v === 'object') return v;
  } catch {}
  return {};
}

export const planLimits = $state<{ byAccount: Record<string, AccountLimits>; busy: Record<string, boolean> }>({
  byAccount: load(),
  busy: {},
});

/** Limit warnings the user dismissed (this run of Wren only; they come back after a reset). */
export const dismissedWarnings = $state<Record<string, boolean>>({});

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify($state.snapshot(planLimits.byAccount)));
  } catch {}
}

/**
 * Ask the account's CLI for fresh numbers. `minAgeMs` skips the call when the
 * numbers we have (from a read or from a reply) are newer than that.
 */
export async function refreshLimits(accountId: string, provider: Provider, minAgeMs = 0): Promise<void> {
  if (provider === 'antigravity' || planLimits.busy[accountId]) return;
  const old = planLimits.byAccount[accountId];
  const fresh = !!old && Date.now() - old.checkedAt < minAgeMs;
  if (minAgeMs && fresh && (!old.error || old.windows.length)) return;
  planLimits.busy[accountId] = true;
  try {
    const l = await invoke<PlanLimits>('plan_limits', { accountId });
    planLimits.byAccount[accountId] = { ...l, checkedAt: Date.now(), error: null, from: 'read' };
  } catch (e) {
    // Keep the numbers we have NOW (not the ones from before the read): a reply
    // may have brought new ones while this slow read was running.
    const cur = planLimits.byAccount[accountId];
    planLimits.byAccount[accountId] = {
      ...(cur ?? { windows: [], daily: null, plan: null, status: null }),
      checkedAt: cur?.checkedAt ?? Date.now(),
      error: String(e),
    };
  } finally {
    planLimits.busy[accountId] = false;
    save();
  }
}

/** New numbers that arrived during a conversation (Claude's rate_limit_event). */
export function applyLimitEvent(accountId: string, l: PlanLimits) {
  const old = planLimits.byAccount[accountId];
  const windows = [...(old?.windows ?? [])];
  for (const w of l.windows) {
    const i = windows.findIndex((x) => x.key === w.key);
    if (i >= 0) windows[i] = w;
    else windows.push(w);
  }
  planLimits.byAccount[accountId] = {
    windows,
    daily: old?.daily ?? null,
    plan: l.plan ?? old?.plan ?? null,
    status: l.status ?? old?.status ?? null,
    checkedAt: Date.now(),
    error: null,
    from: 'reply',
  };
  save();
}

/** A window whose reset time has passed is back at 0%. */
export function usedNow(w: { used_percent: number; resets_at: number | null }, now = Date.now()): number {
  if (w.resets_at && w.resets_at * 1000 <= now) return 0;
  return w.used_percent;
}

export type Level = 'ok' | 'warn' | 'critical';
export const levelOf = (pct: number): Level => (pct >= 95 ? 'critical' : pct >= 80 ? 'warn' : 'ok');

/** "in 2h 13m", "in 3 days", "Fri 09:00" */
export function resetText(resetsAt: number | null, now = Date.now()): string {
  if (!resetsAt) return '';
  const ms = resetsAt * 1000 - now;
  if (ms <= 0) return 'reset';
  const mins = Math.round(ms / 60000);
  if (mins < 60) return `resets in ${mins}m`;
  if (mins < 24 * 60) return `resets in ${Math.floor(mins / 60)}h ${mins % 60}m`;
  const d = new Date(resetsAt * 1000);
  return `resets ${d.toLocaleDateString([], { weekday: 'short' })} ${d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
}

/** "from your last reply, 3 min ago" / "checked 3 min ago" */
export function sourceText(l: { checkedAt: number; from?: 'reply' | 'read' }, now = Date.now()): string {
  return l.from === 'reply' ? `from your last reply, ${agoText(l.checkedAt, now)}` : `checked ${agoText(l.checkedAt, now)}`;
}

/** "checked 3 min ago" */
export function agoText(t: number, now = Date.now()): string {
  const m = Math.round((now - t) / 60000);
  if (m < 1) return 'just now';
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  return new Date(t).toLocaleDateString();
}
