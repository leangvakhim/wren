<script lang="ts">
  import type { Question } from './types';

  let {
    who,
    questions,
    status,
    answers,
    onsubmit,
  }: {
    /** "Claude Code", "Codex", … */
    who: string;
    questions: Question[];
    status: 'waiting' | 'answered' | 'skipped' | 'expired';
    /** The answers that were sent (shown once answered). */
    answers?: Record<string, string[]>;
    /** Answers by question id, or null when skipped. */
    onsubmit: (answers: Record<string, string[]> | null) => void;
  } = $props();

  const uid = Math.random().toString(36).slice(2, 8);
  /** Picked option labels per question. */
  let picked = $state<Record<string, string[]>>({});
  /** Typed "Other" answers per question. */
  let typed = $state<Record<string, string>>({});
  let sending = $state(false);

  function toggle(q: Question, label: string) {
    const cur = picked[q.id] ?? [];
    if (q.multi) picked[q.id] = cur.includes(label) ? cur.filter((l) => l !== label) : [...cur, label];
    else {
      picked[q.id] = [label];
      // One choice only: a picked option replaces a typed answer.
      typed[q.id] = '';
    }
  }

  function onType(q: Question, v: string) {
    typed[q.id] = v;
    if (!q.multi && v.trim()) picked[q.id] = [];
  }

  function collect(): Record<string, string[]> {
    const out: Record<string, string[]> = {};
    for (const q of questions) {
      const a = [...(picked[q.id] ?? [])];
      const t = (typed[q.id] ?? '').trim();
      if (t) a.push(t);
      if (a.length) out[q.id] = a;
    }
    return out;
  }

  const answeredCount = $derived(Object.keys(collect()).length);

  function submit() {
    if (sending || !answeredCount) return;
    sending = true;
    onsubmit(collect());
  }

  function skip() {
    if (sending) return;
    sending = true;
    onsubmit(null);
  }

  function shown(q: Question): string {
    const a = answers?.[q.id];
    if (!a?.length) return 'No answer';
    return q.secret ? '••••••' : a.join(', ');
  }
</script>

<div class="qcard" class:waiting={status === 'waiting'} role={status === 'waiting' ? 'form' : 'group'} aria-label={`${who} asks`}>
  <div class="qhead">
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3" /><path d="M12 17h.01" /></svg>
    <span><b>{who}</b> {status !== 'waiting' ? (questions.length > 1 ? 'asked you:' : 'asked:') : questions.length > 1 ? `has ${questions.length} questions for you` : 'has a question for you'}</span>
  </div>

  {#if status === 'waiting'}
    {#each questions as q, qi (q.id)}
      <fieldset class="q">
        <legend>
          {#if q.header}<span class="chip">{q.header}</span>{/if}
          <span class="qtext">{q.question}</span>
          {#if q.multi}<span class="hint">Choose any</span>{/if}
        </legend>
        {#if q.options.length}
          <div class="opts">
            {#each q.options as o, oi (oi)}
              {@const on = (picked[q.id] ?? []).includes(o.label)}
              <label class="opt" class:on>
                <input
                  type={q.multi ? 'checkbox' : 'radio'}
                  name={`q-${uid}-${qi}`}
                  checked={on}
                  onchange={() => toggle(q, o.label)}
                />
                <span class="otext">
                  <span class="olabel">{o.label}</span>
                  {#if o.description}<span class="odesc">{o.description}</span>{/if}
                </span>
              </label>
            {/each}
          </div>
        {/if}
        {#if q.other}
          <input
            class="other"
            type={q.secret ? 'password' : 'text'}
            placeholder={q.options.length ? 'Other: type your own answer' : 'Type your answer'}
            aria-label={q.options.length ? `Other answer for: ${q.question}` : q.question}
            value={typed[q.id] ?? ''}
            oninput={(e) => onType(q, e.currentTarget.value)}
            onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); submit(); } }}
          />
        {/if}
      </fieldset>
    {/each}
    <div class="qbtns">
      <button class="primary" disabled={!answeredCount || sending} onclick={submit}>
        Submit{questions.length > 1 ? ` (${answeredCount}/${questions.length})` : ''}
      </button>
      <button class="ghost" disabled={sending} onclick={skip} title="Let the AI continue without your answers">Skip</button>
    </div>
  {:else if status === 'answered'}
    <ul class="summary">
      {#each questions as q (q.id)}
        <li><span class="sq">{q.header || q.question}</span><span class="arrow" aria-hidden="true">→</span><span class="sa" class:none={!answers?.[q.id]?.length}>{shown(q)}</span></li>
      {/each}
    </ul>
  {:else if status === 'skipped'}
    <p class="qstate">Skipped. The AI continued without your answers.</p>
  {:else}
    <p class="qstate">No longer waiting (the task ended or was stopped)</p>
  {/if}
</div>

<style>
  .qcard { border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; background: var(--surface-2); display: flex; flex-direction: column; gap: 10px; font-size: 13px; }
  .qcard.waiting { border-color: var(--accent-fill); box-shadow: 0 0 0 1px var(--accent-fill); }
  .qhead { display: flex; align-items: center; gap: 8px; }
  .qhead svg { color: var(--accent); flex-shrink: 0; }
  .q { border: 0; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  legend { padding: 0; margin-bottom: 6px; display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px; }
  .chip { font-size: 11px; font-weight: 600; padding: 1px 7px; border-radius: 999px; background: color-mix(in oklab, var(--accent-fill) 18%, transparent); color: var(--text); }
  .qtext { font-weight: 600; }
  .hint { color: var(--muted); font-size: 12px; }
  .opts { display: flex; flex-direction: column; gap: 4px; }
  .opt { display: flex; align-items: flex-start; gap: 8px; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); cursor: pointer; }
  .opt:hover { border-color: var(--accent-fill); }
  .opt.on { border-color: var(--accent); background: color-mix(in oklab, var(--accent-fill) 10%, var(--bg)); }
  .opt input { margin: 2px 0 0; accent-color: var(--accent); flex-shrink: 0; }
  .otext { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .olabel { overflow-wrap: anywhere; }
  .odesc { color: var(--muted); font-size: 12px; overflow-wrap: anywhere; }
  .other { width: 100%; box-sizing: border-box; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font: inherit; }
  .other:focus { outline: 2px solid var(--accent); outline-offset: -1px; }
  .qbtns { display: flex; gap: 8px; flex-wrap: wrap; }
  .qbtns button { min-height: 32px; }
  .summary { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .summary li { display: flex; gap: 6px; align-items: baseline; flex-wrap: wrap; }
  .sq { color: var(--muted); }
  .arrow { color: var(--muted); }
  .sa { font-weight: 600; overflow-wrap: anywhere; }
  .sa.none { font-weight: 400; color: var(--muted); font-style: italic; }
  .qstate { margin: 0; font-size: 12.5px; font-weight: 600; color: var(--muted); }
</style>
