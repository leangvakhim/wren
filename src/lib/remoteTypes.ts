/**
 * Messages between Wren's window and a paired phone (inside the encrypted
 * phone link; see src-tauri/src/remote). Shared by the desktop UI
 * (remote.svelte.ts) and the phone page (phone/src).
 */
import type { ConversationMeta, Mode, ModelInfo, Msg, Permission, Provider, Question } from './types';

/** What the phone shows about one pane. */
export interface PhonePaneMeta {
  /** Wren's pane slot (0-3); stays the same while the pane is open. */
  slot: number;
  /** Position on the computer's screen (0 = first). */
  pos: number;
  provider: Provider;
  accountId: string;
  accountName: string;
  permission: Permission;
  mode: Mode | null;
  model: string;
  effort: string;
  /** "Opus 5.5", "GPT-6.2-Sol"… ('' = the CLI's default). */
  modelLabel: string;
  cwd: string;
  title: string;
  convId: string | null;
  busy: boolean;
  /** ms, when the current task started (0 = not working). */
  startedAt: number;
  thinking: boolean;
  waitingFor: 'approval' | 'question' | null;
  /** Waiting for a usage reset before continuing by itself. */
  waitingLimit: { until: number; known: boolean } | null;
  /** The account hit its usage limit: the phone can pick another account. */
  limitHit: boolean;
  /** Why the pane can't work right now (CLI missing, no account…). */
  problem: string | null;
  totals: { input: number; output: number; cost: number; hasCost: boolean };
}

export interface PhoneFile {
  name: string;
  size: number;
  kind: 'image' | 'file';
}

/** A chat message as the phone gets it (long outputs shortened). */
export interface PhoneMsg {
  id: number;
  role: Msg['role'];
  text: string;
  detail?: string;
  result?: string;
  isError?: boolean;
  files?: PhoneFile[];
  durationMs?: number;
  step?: boolean;
  tool?: string;
  approvalState?: Msg['approvalState'];
  autoBy?: string;
  allowSession?: boolean;
  questions?: Question[];
  answers?: Record<string, string[]>;
  questionState?: Msg['questionState'];
}

export interface PhoneAccount {
  id: string;
  provider: Provider;
  name: string;
  usable: boolean;
}

/** Everything the phone's home screen needs. */
export interface PhoneApp {
  t: 'app';
  panes: PhonePaneMeta[];
  /** Panes can still be added on the computer (at most 4). */
  canAdd: boolean;
  accounts: PhoneAccount[];
  installed: Record<Provider, boolean>;
  /** Project folders used recently (newest first). */
  folders: string[];
  defaultFolder: string;
  history: ConversationMeta[];
}

/** One pane's chat: `ids` = every message in order; `upd` = the ones that changed. */
export interface PhonePane {
  t: 'pane';
  slot: number;
  meta: PhonePaneMeta;
  ids: number[];
  upd: PhoneMsg[];
  /** The phone should forget what it had for this pane. */
  reset?: boolean;
}

export interface PhoneReady {
  t: 'ready';
  host: string;
  version: string;
  vapid: string;
  device: { id: string; name: string };
  push_kinds: string[];
}

export interface PhoneRes {
  t: 'res';
  id: number;
  ok: boolean;
  data?: unknown;
  error?: string;
}

export type FromWren = PhoneApp | PhonePane | PhoneReady | PhoneRes;

/** Changes the phone can make to a pane. */
export interface PanePatch {
  provider?: Provider;
  accountId?: string;
  permission?: Permission;
  model?: string;
  effort?: string;
  mode?: Mode | null;
  cwd?: string;
}

export interface NewTask {
  provider: Provider;
  accountId: string;
  permission: Permission;
  model: string;
  effort: string;
  mode: Mode | null;
  cwd: string;
}

/** An attachment the phone uploaded (a private copy in Wren's folder). */
export interface Uploaded {
  path: string;
  name: string;
  size: number;
  mime: string;
  kind: 'image' | 'file';
}

/** Requests from the phone: {t:'req', id, op, ...}. */
export type PhoneRequest =
  | { op: 'watch'; slot: number | null }
  | { op: 'send'; slot: number; text: string; files?: Uploaded[] }
  | { op: 'stop'; slot: number }
  | { op: 'approve'; slot: number; msg: number; decision: 'allow' | 'allow_session' | 'deny' | 'retry' | 'keep' }
  | { op: 'answer'; slot: number; msg: number; answers: Record<string, string[]> | null }
  | { op: 'new_chat'; slot: number }
  | { op: 'close'; slot: number }
  | { op: 'config'; slot: number; patch: PanePatch }
  | { op: 'limit'; slot: number; action: 'switch' | 'continue' | 'cancel' | 'dismiss'; accountId?: string }
  | { op: 'new_task'; config: NewTask; text: string; files?: Uploaded[] }
  | { op: 'open_chat'; convId: string }
  | { op: 'models'; provider: Provider; accountId: string };

export type ModelList = ModelInfo[];
