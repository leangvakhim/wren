import { invoke } from '@tauri-apps/api/core';
import type { GithubAccount } from './types';

/** Shared, reactive list of GitHub accounts (signed in through gh). */
export const github = $state<{ list: GithubAccount[]; loaded: boolean; gh: string | null | undefined }>({
  list: [],
  loaded: false,
  gh: undefined, // where gh is installed; null = not installed
});

export async function loadGithub() {
  const [list, gh] = await Promise.all([invoke<GithubAccount[]>('list_github'), invoke<string | null>('github_cli')]);
  github.list = list;
  github.gh = gh;
  github.loaded = true;
}

/** Ask gh whether this account is signed in, and as whom. */
export async function checkGithub(id: string) {
  const a = await invoke<GithubAccount>('check_github', { id });
  const i = github.list.findIndex((x) => x.id === id);
  if (i >= 0) github.list[i] = a;
  return a;
}

export async function checkAllGithub() {
  if (!github.gh) return;
  await Promise.all(github.list.map((a) => checkGithub(a.id).catch(() => {})));
}

export const findGithub = (id: string | undefined) => (id ? github.list.find((a) => a.id === id) : undefined);

export function githubLabel(a: GithubAccount) {
  const who = a.login ? ` (@${a.login})` : '';
  const st = a.status === 'pending' ? ' (not signed in)' : a.status === 'signed_out' ? ' (signed out)' : '';
  return `${a.name}${who}${st}`;
}

/** How to install gh on this computer. */
export function installHint(): { command: string | null; url: string } {
  const mac = navigator.userAgent.includes('Mac');
  return { command: mac ? 'brew install gh' : null, url: 'https://cli.github.com' };
}
