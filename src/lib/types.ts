export type Provider = 'claude' | 'codex' | 'antigravity';
export type Permission = 'manual' | 'auto' | 'plan' | 'full';

/** Older saves used 'ask' (Read only) and 'edits' (Auto-edit). */
export function normalizePermission(p: string | null | undefined): Permission {
  if (p === 'auto' || p === 'edits') return 'auto';
  if (p === 'plan' || p === 'full') return p;
  return 'manual';
}

export interface PaneConfig {
  provider: Provider;
  /** Project folder the CLI works in ('' = not chosen yet). */
  cwd: string;
  permission: Permission;
  /** Which account's login this pane uses. */
  accountId: string;
  /** Model id for the CLI ('' = the CLI's default). */
  model?: string;
  /** Effort tier ('' = the model's default). */
  effort?: string;
  /** Study / Deep research (null = normal). */
  mode?: Mode | null;
  /** GitHub account gh and git use in this pane ('' = the computer's normal setup). */
  githubAccountId?: string;
}

/** Study / Deep research (see the "+" menu in the message box). */
export type Mode = 'study' | 'research';

export const MODES: Record<Mode, { label: string; hint: string }> = {
  study: { label: 'Study', hint: 'Tutor mode: explains step by step, gives hints before answers and checks your understanding.' },
  research: {
    label: 'Deep research',
    hint: 'Searches the web widely, checks sources against each other and writes a cited report (saved in the folder). Uses more of your plan.',
  },
};

/** A file attached in Wren (a private copy in Wren's data folder). */
export interface Attachment {
  path: string;
  name: string;
  size: number;
  mime: string;
  /** 'image' = shown as a picture; 'file' = shown as a chip */
  kind: 'image' | 'file';
  /** data: URL thumbnail (images only) */
  preview: string | null;
}

/** What a sent message remembers about each attachment. */
export type SentFile = Omit<Attachment, 'preview'>;

/** One plan-limit window (see src-tauri/src/events.rs). */
export interface LimitWindow {
  key: string;
  label: string;
  /** 0-100 (can go above 100). */
  used_percent: number;
  /** unix seconds */
  resets_at: number | null;
}

export interface PlanLimits {
  windows: LimitWindow[];
  /** Tokens per day counted by the AI company (Codex only). */
  daily: { date: string; tokens: number }[] | null;
  plan: string | null;
  status: string | null;
}

export interface AccountLimits extends PlanLimits {
  /** ms */
  checkedAt: number;
  /** Why the last read failed (the numbers above are then older). */
  error: string | null;
  /** Where the numbers came from: a reply in a pane ('reply') or a read from the CLI ('read'). */
  from?: 'reply' | 'read';
}

export interface ModelInfo {
  /** Sent to the CLI (Claude: the alias, e.g. "opus", so it follows the newest model). */
  id: string;
  /** Full name with version, e.g. "Opus 5.5", "GPT-6.2-Sol", "Gemini 4 Carbon". */
  label: string;
  efforts: string[];
  default_effort: string | null;
  /** One line about the model. */
  description: string | null;
  /** The exact model an alias points to now (e.g. claude-opus-5-5). */
  resolved: string | null;
  /** The model the CLI uses when none is chosen. */
  is_default: boolean;
}

export interface Totals {
  input: number;
  output: number;
  cost: number;
  hasCost: boolean;
  unpriced: boolean;
}

/** Usage of one answered turn (for the Usage window). */
export interface UsageRecord {
  t: number;
  provider: Provider;
  accountId: string;
  model: string;
  /** Input tokens that were not cached (includes cache writes). */
  input: number;
  cached: number;
  output: number;
  /** USD (reported by the CLI or estimated); null = unknown price. */
  cost: number | null;
}

/** A saved conversation (stored only on this computer). */
export interface Conversation {
  id: string;
  title: string;
  provider: Provider;
  accountId: string;
  cwd: string;
  permission: Permission;
  model: string;
  effort: string;
  mode?: Mode | null;
  sessionId: string | null;
  createdAt: number;
  updatedAt: number;
  messages: Msg[];
  totals: Totals;
  usage?: UsageRecord[];
}

export type ConversationMeta = Pick<Conversation, 'id' | 'title' | 'provider' | 'accountId' | 'cwd' | 'createdAt' | 'updatedAt'>;

export type AccountStatus = 'unknown' | 'pending' | 'signed_in' | 'signed_out';

export interface Account {
  id: string;
  provider: Provider;
  name: string;
  status: AccountStatus;
  email: string | null;
  is_default: boolean;
}

/** A GitHub account, signed in through the GitHub CLI (gh). */
export interface GithubAccount {
  id: string;
  name: string;
  status: AccountStatus;
  /** GitHub username, known after sign-in. */
  login: string | null;
  is_default: boolean;
}

export interface AccountsView {
  accounts: Account[];
  active_antigravity: string | null;
}

export type LoginEvent =
  | { kind: 'output'; data: string }
  | { kind: 'url'; url: string }
  | { kind: 'exited'; code: number | null };

export interface Usage {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
}

/** Events sent by the Rust backend (see src-tauri/src/events.rs). */
export type UiEvent =
  | { kind: 'plan_limits'; limits: PlanLimits }
  | { kind: 'thinking' }
  | { kind: 'approval'; id: string; tool: string; title: string; detail: string | null; allow_session: boolean }
  | { kind: 'question'; id: string; questions: Question[] }
  | { kind: 'notice'; text: string }
  | { kind: 'session_started'; session_id: string; model: string | null }
  | { kind: 'text_delta'; text: string }
  | { kind: 'text_end'; full: string | null }
  | { kind: 'tool_use'; name: string; summary: string }
  | { kind: 'tool_result'; output: string; is_error: boolean }
  | { kind: 'turn_done'; usage: Usage; cost_usd: number | null; error: string | null }
  | { kind: 'error'; message: string }
  | { kind: 'log'; line: string }
  | { kind: 'exited'; code: number | null };

export interface Msg {
  id: number;
  /** notice = something Wren handled for you this turn ('details' = old raw logs, no longer shown). */
  role: 'user' | 'assistant' | 'tool' | 'error' | 'info' | 'notice' | 'details' | 'approval' | 'question' | 'divider';
  text: string;
  /** Tool calls: the command/file/query. */
  detail?: string;
  /** Tool calls: the output, once it arrives. */
  result?: string;
  isError?: boolean;
  /** User messages: attached files (private copies in Wren's folder). */
  files?: SentFile[];
  /** Older saves: attached image paths. */
  images?: string[];
  /** User messages: how long the AI worked on this turn (ms), for "Worked for 3m 33s". */
  durationMs?: number;
  /** Approval cards: the CLI's request id, the tool, and what happened. */
  approvalId?: string;
  tool?: string;
  approvalState?: 'waiting' | 'allowed' | 'allowed_session' | 'denied' | 'expired' | 'refused' | 'retried' | 'kept';
  allowSession?: boolean;
  /** Question forms: the questions, and the user's answers (by question id) once sent. */
  questions?: Question[];
  answers?: Record<string, string[]>;
  questionState?: 'waiting' | 'answered' | 'skipped' | 'expired';
}

/** One question of a question form the AI asked (Claude AskUserQuestion, Codex request_user_input). */
export interface Question {
  /** Key the answer is sent back under. */
  id: string;
  header: string;
  question: string;
  /** Empty = free-text question. */
  options: { label: string; description: string | null }[];
  multi: boolean;
  /** A typed answer is allowed. */
  other: boolean;
  secret: boolean;
}

export const PROVIDERS: Record<Provider, { label: string; bin: string }> = {
  claude: { label: 'Claude Code', bin: 'claude' },
  codex: { label: 'Codex', bin: 'codex' },
  antigravity: { label: 'Antigravity', bin: 'agy' },
};

export const PERMISSIONS: { value: Permission; label: string; hint: string }[] = [
  { value: 'manual', label: 'Manual', hint: 'Asks before it edits files or runs commands. You allow or deny each request in the chat.' },
  { value: 'auto', label: 'Auto', hint: "Decides by itself: safe actions run, risky ones are blocked (the AI's own auto mode). Doesn't ask you." },
  { value: 'plan', label: 'Plan', hint: 'Researches and proposes a plan. Changes nothing.' },
  { value: 'full', label: 'Full access', hint: 'Edits files AND runs any command without asking or checking. Use with care.' },
];

export const defaultAccountId = (p: Provider) => `default-${PROVIDERS[p].bin}`;

export const DEFAULT_CONFIG: PaneConfig = { provider: 'claude', cwd: '', permission: 'manual', accountId: '', model: '', effort: '', mode: null, githubAccountId: '' };
