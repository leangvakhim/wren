<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import type { EditorView } from '@codemirror/view';
  import type { Compartment } from '@codemirror/state';

  /**
   * The browser panel's Code tab: the page's file with colors and line
   * numbers (CodeMirror, loaded only when the tab is first opened).
   * `onedit` fires for the user's own typing, never for `setText`.
   */
  let {
    text,
    name,
    editable,
    wrap,
    onedit,
  }: { text: string; name: string; editable: boolean; wrap: boolean; onedit: (text: string) => void } = $props();

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | undefined;
  let parts: { editable: Compartment; wrap: Compartment; lang: Compartment } | undefined;
  let cm: Awaited<ReturnType<typeof load>> | undefined;
  /** Set while Wren itself replaces the text (not the user's typing). */
  let silent = false;
  let failed = $state('');

  async function load() {
    const [core, view, state, language, highlight, html, css, js, commands] = await Promise.all([
      import('codemirror'),
      import('@codemirror/view'),
      import('@codemirror/state'),
      import('@codemirror/language'),
      import('@lezer/highlight'),
      import('@codemirror/lang-html'),
      import('@codemirror/lang-css'),
      import('@codemirror/lang-javascript'),
      import('@codemirror/commands'),
    ]);
    return { core, view, state, language, highlight, html, css, js, commands };
  }

  function languageFor(file: string) {
    if (!cm) return [];
    const ext = file.split('.').pop()?.toLowerCase() ?? '';
    if (ext === 'css') return cm.css.css();
    if (ext === 'js' || ext === 'mjs' || ext === 'json') return cm.js.javascript();
    if (['txt', 'md', 'csv'].includes(ext)) return [];
    // .html/.svg and web pages (whose address may not end in .html)
    return cm.html.html();
  }

  /** Replace the whole text (the file changed on disk), keeping the cursor where it can. */
  export function setText(next: string) {
    if (!view || view.state.doc.toString() === next) return;
    const head = Math.min(view.state.selection.main.head, next.length);
    silent = true;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next }, selection: { anchor: head } });
    silent = false;
  }

  export function getText(): string {
    return view ? view.state.doc.toString() : text;
  }

  onMount(async () => {
    try {
      cm = await load();
    } catch (e) {
      failed = `The code view couldn't load: ${e}`;
      return;
    }
    if (!host) return;
    const { EditorView, keymap } = cm.view;
    const { EditorState, Compartment } = cm.state;
    const t = cm.highlight.tags;
    const colors = cm.language.HighlightStyle.define([
      { tag: [t.keyword, t.controlKeyword, t.moduleKeyword, t.operatorKeyword, t.definitionKeyword], color: 'var(--code-kw)' },
      { tag: [t.tagName], color: 'var(--code-tag)' },
      { tag: [t.attributeName, t.propertyName], color: 'var(--code-attr)' },
      { tag: [t.string, t.attributeValue, t.special(t.string), t.regexp], color: 'var(--code-str)' },
      { tag: [t.number, t.bool, t.null, t.atom, t.unit], color: 'var(--code-num)' },
      { tag: [t.comment, t.lineComment, t.blockComment], color: 'var(--code-comment)', fontStyle: 'italic' },
      { tag: [t.function(t.variableName), t.function(t.propertyName)], color: 'var(--code-fn)' },
      { tag: [t.typeName, t.className, t.labelName], color: 'var(--code-type)' },
      { tag: [t.operator, t.punctuation, t.angleBracket, t.bracket, t.separator], color: 'var(--code-punct)' },
      { tag: t.invalid, color: 'var(--danger)' },
    ]);
    const look = EditorView.theme({
      '&': { height: '100%', color: 'var(--text)', backgroundColor: 'var(--bg)', fontSize: '12.5px' },
      '.cm-scroller': { fontFamily: 'var(--mono)', lineHeight: '1.55' },
      '.cm-content': { caretColor: 'var(--text)' },
      '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--text)' },
      '.cm-gutters': { backgroundColor: 'var(--surface-2)', color: 'var(--muted)', borderRight: '1px solid var(--border)' },
      '.cm-activeLine': { backgroundColor: 'var(--code-line)' },
      '.cm-activeLineGutter': { backgroundColor: 'var(--code-line)', color: 'var(--text)' },
      '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection': {
        backgroundColor: 'var(--code-sel) !important',
      },
      '.cm-panels': { backgroundColor: 'var(--surface-2)', color: 'var(--text)' },
      '.cm-panels input, .cm-panels button': { font: 'inherit', fontSize: '12px' },
      '.cm-searchMatch': { backgroundColor: 'var(--code-match)' },
      '.cm-foldPlaceholder': { backgroundColor: 'var(--surface-2)', border: '1px solid var(--border)', color: 'var(--muted)' },
      '.cm-tooltip': { backgroundColor: 'var(--surface)', border: '1px solid var(--border)', color: 'var(--text)' },
    });
    parts = { editable: new Compartment(), wrap: new Compartment(), lang: new Compartment() };
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: text,
        extensions: [
          cm.core.basicSetup,
          // Tab indents; Esc then Tab leaves the editor (CodeMirror's rule).
          keymap.of([cm.commands.indentWithTab]),
          cm.language.syntaxHighlighting(colors),
          look,
          parts.lang.of(languageFor(name)),
          parts.editable.of([EditorState.readOnly.of(!editable), EditorView.editable.of(true)]),
          parts.wrap.of(wrap ? EditorView.lineWrapping : []),
          EditorView.updateListener.of((u) => {
            if (u.docChanged && !silent) onedit(u.state.doc.toString());
          }),
          EditorView.contentAttributes.of({ 'aria-label': `Code of ${name}`, spellcheck: 'false', autocorrect: 'off', autocapitalize: 'off' }),
        ],
      }),
    });
  });

  // Follow the props after the first load.
  $effect(() => {
    const ro = !editable;
    if (view && parts && cm) {
      view.dispatch({ effects: parts.editable.reconfigure([cm.state.EditorState.readOnly.of(ro), cm.view.EditorView.editable.of(true)]) });
    }
  });
  $effect(() => {
    const w = wrap;
    if (view && parts && cm) view.dispatch({ effects: parts.wrap.reconfigure(w ? cm.view.EditorView.lineWrapping : []) });
  });
  $effect(() => {
    const n = name;
    if (view && parts && cm) view.dispatch({ effects: parts.lang.reconfigure(languageFor(n)) });
  });

  onDestroy(() => view?.destroy());
</script>

<div class="code" bind:this={host}>
  {#if failed}<p class="failed">{failed}</p>{/if}
</div>

<style>
  .code {
    position: absolute;
    inset: 0;
    overflow: hidden;
    --code-kw: #cf222e;
    --code-tag: #116329;
    --code-attr: #0550ae;
    --code-str: #0a3069;
    --code-num: #0550ae;
    --code-comment: #6e7781;
    --code-fn: #8250df;
    --code-type: #953800;
    --code-punct: #57606a;
    --code-line: rgba(23, 24, 28, 0.045);
    --code-sel: rgba(227, 155, 75, 0.3);
    --code-match: rgba(250, 178, 25, 0.35);
  }
  :global([data-theme='dark']) .code {
    --code-kw: #ff7b72;
    --code-tag: #7ee787;
    --code-attr: #79c0ff;
    --code-str: #a5d6ff;
    --code-num: #79c0ff;
    --code-comment: #8b949e;
    --code-fn: #d2a8ff;
    --code-type: #ffa657;
    --code-punct: #9198a1;
    --code-line: rgba(236, 236, 232, 0.05);
    --code-sel: rgba(227, 155, 75, 0.32);
    --code-match: rgba(250, 178, 25, 0.28);
  }
  .failed { margin: 16px; color: var(--danger); font-size: 13px; }
</style>
