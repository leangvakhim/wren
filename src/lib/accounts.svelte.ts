import { invoke } from '@tauri-apps/api/core';
import type { Account, AccountsView, Provider } from './types';

/** Shared, reactive list of accounts (any number per AI). */
export const accounts = $state<{ list: Account[]; activeAgy: string | null; loaded: boolean }>({
  list: [],
  activeAgy: null,
  loaded: false,
});

export async function loadAccounts() {
  const v = await invoke<AccountsView>('list_accounts');
  accounts.list = v.accounts;
  accounts.activeAgy = v.active_antigravity;
  accounts.loaded = true;
}

/** Ask the CLI whether this account is signed in (Claude Code / Codex). */
export async function checkAccount(id: string) {
  const a = await invoke<Account>('check_account', { id });
  const i = accounts.list.findIndex((x) => x.id === id);
  if (i >= 0) accounts.list[i] = a;
  return a;
}

export async function checkAll(installed: Partial<Record<Provider, string | null>>) {
  await Promise.all(
    accounts.list
      .filter((a) => a.provider !== 'antigravity' && installed[a.provider])
      .map((a) => checkAccount(a.id).catch(() => {})),
  );
}

export const accountsFor = (p: Provider) => accounts.list.filter((a) => a.provider === p);

export const findAccount = (id: string) => accounts.list.find((a) => a.id === id);

/** The account a pane should use for this AI: a usable one (this computer's login first), or '' if Wren has none. */
export function bestAccountId(p: Provider): string {
  const list = accountsFor(p);
  const usable = list.filter((a) => isUsable(a));
  return (usable.find((a) => a.is_default) ?? usable[0] ?? list[0])?.id ?? '';
}

/** Whether this account can be used right now. */
export function isUsable(a: Account | undefined): boolean {
  if (!a) return false;
  if (a.provider === 'antigravity') return a.is_default || accounts.activeAgy === a.id;
  return a.status === 'signed_in' || (a.is_default && a.status === 'unknown');
}

export const STATUS_LABEL: Record<Account['status'], string> = {
  unknown: 'Not checked',
  pending: 'Not signed in yet',
  signed_in: 'Signed in',
  signed_out: 'Signed out',
};

export function accountLabel(a: Account): string {
  const who = a.email ? ` · ${a.email}` : '';
  let state = '';
  if (a.provider === 'antigravity' && !a.is_default) {
    state = accounts.activeAgy === a.id ? ' (active)' : ' (not active)';
  } else if (a.status === 'pending' || a.status === 'signed_out') {
    state = ' (not signed in)';
  }
  return `${a.name}${who}${state}`;
}
