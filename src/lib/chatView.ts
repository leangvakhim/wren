import type { Msg } from './types';

/** The parts of a chat message the layout needs (desktop Msg and the phone's PhoneMsg both fit). */
export type ChatMsg = Pick<Msg, 'id' | 'role' | 'text' | 'detail' | 'step' | 'tool' | 'durationMs' | 'approvalState' | 'autoBy' | 'questions' | 'questionState'>;

export type Item<M extends ChatMsg> =
  | { kind: 'msg'; m: M }
  | { kind: 'group'; key: number; steps: M[]; live: boolean; durationMs: number | undefined };

/** An approval card or question form that needs nothing more from you. */
export function settledCard(m: ChatMsg): boolean {
  if (m.role === 'approval') return m.approvalState !== 'waiting' && m.approvalState !== 'refused';
  if (m.role === 'question') return m.questionState !== 'waiting';
  return false;
}

/**
 * Each turn = the user's message, one group with the work (tool calls and the
 * AI's notes between them), then everything else in order (the answer,
 * approval cards, errors, notes).
 */
export function groupTurns<M extends ChatMsg>(messages: M[], busy: boolean): Item<M>[] {
  const out: Item<M>[] = [];
  const segs: { user: M | null; rest: M[] }[] = [{ user: null, rest: [] }];
  for (const m of messages) {
    if (m.role === 'user') segs.push({ user: m, rest: [] });
    else segs[segs.length - 1].rest.push(m);
  }
  segs.forEach((seg, i) => {
    const last = i === segs.length - 1;
    if (seg.user) out.push({ kind: 'msg', m: seg.user });
    // An AI message is a step if more tool work follows it in this turn.
    const lastTool = seg.rest.map((m) => m.role).lastIndexOf('tool');
    // Answered approval cards and question forms move into the steps too, so the chat stays clean.
    const steps = seg.user
      ? seg.rest.filter((m, j) => m.role === 'tool' || (m.role === 'assistant' && j < lastTool) || settledCard(m) || (m.role === 'info' && m.step))
      : [];
    const live = last && busy && !!seg.user;
    if (seg.user && (steps.length || live)) {
      out.push({ kind: 'group', key: seg.user.id, steps, live, durationMs: seg.user.durationMs });
    }
    for (const m of seg.rest) if (!steps.includes(m) && m.role !== 'details') out.push({ kind: 'msg', m });
  });
  return out;
}

/** One-line label for an answered card in the steps. */
export function cardLabel(m: ChatMsg): { label: string; subject: string; bad: boolean } {
  if (m.role === 'question') {
    const n = m.questions?.length ?? 0;
    const subject = (m.questions ?? []).map((q) => q.header || q.question).join(' · ');
    const label =
      m.questionState === 'answered' ? `Answered ${n === 1 ? 'a question' : `${n} questions`}` : m.questionState === 'skipped' ? 'Skipped questions' : 'Questions not answered';
    return { label, subject, bad: false };
  }
  const plan = m.tool === 'ExitPlanMode';
  const subject = plan ? 'Plan' : `${m.tool}: ${m.text.split('\n')[0]}`;
  switch (m.approvalState) {
    case 'allowed':
      return { label: plan ? 'Started the plan' : 'Allowed', subject, bad: false };
    case 'allowed_session':
      return { label: 'Allowed for this chat', subject, bad: false };
    case 'auto_allowed':
      return { label: `Allowed by ${m.autoBy ?? 'the new permission'}`, subject, bad: false };
    case 'denied':
      return { label: plan ? 'Kept planning' : 'Denied', subject, bad: true };
    case 'retried':
      return { label: 'Retried with Auto', subject, bad: false };
    case 'kept':
      return { label: 'Kept refused', subject, bad: true };
    default:
      return { label: 'Not answered', subject, bad: false };
  }
}

/** "Ran a command", "Read a file", … for a tool call. */
export function stepLabel(m: ChatMsg): { label: string; subject: string } {
  const name = m.text;
  const d = m.detail ?? '';
  const file = d.split(/[\\/]/).filter(Boolean).pop() ?? d;
  const n = name.toLowerCase();
  if (['bash', 'shell', 'run_command', 'powershell', 'exec_command'].includes(n)) return { label: 'Ran a command', subject: '' };
  if (['read', 'view_file', 'read_file', 'notebookread'].includes(n)) return { label: 'Read a file', subject: file };
  if (['write', 'write_to_file', 'create_file'].includes(n)) return { label: 'Wrote a file', subject: file };
  if (['edit', 'multiedit', 'notebookedit', 'replace_file_content', 'replace', 'apply_patch'].includes(n))
    return { label: n === 'edit' && d.includes(',') ? 'Edited files' : 'Edited a file', subject: n === 'edit' && d.includes(',') ? '' : file };
  if (['grep', 'glob', 'grep_search', 'find_by_name', 'list_dir', 'ls'].includes(n)) return { label: 'Searched files', subject: d };
  if (['websearch', 'web_search', 'search_web'].includes(n)) return { label: 'Searched the web', subject: d };
  if (['webfetch', 'read_url_content', 'fetch'].includes(n)) return { label: 'Read a web page', subject: d };
  if (['agent', 'task'].includes(n)) return { label: 'Ran a sub-agent', subject: d };
  if (['todowrite', 'taskcreate', 'taskupdate'].includes(n)) return { label: 'Updated tasks', subject: '' };
  return { label: `Used ${name}`, subject: d };
}

export function fmtDuration(ms: number | undefined): string {
  if (ms === undefined) return '';
  const s = Math.max(1, Math.round(ms / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

export const BIRD_WORDS = ['Pecking', 'Foraging', 'Figuring', 'Nesting', 'Preening', 'Scouting', 'Hopping', 'Fluttering', 'Gathering twigs'];
