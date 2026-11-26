import { marked } from 'marked';
import DOMPurify from 'dompurify';

marked.setOptions({ gfm: true, breaks: false });

/**
 * Model output can repeat text from web pages or files, so it is untrusted.
 * Render markdown, then strip anything active or anything that would load
 * from the network (images, frames, forms, styles).
 */
export function renderMarkdown(text: string): string {
  const html = marked.parse(text, { async: false }) as string;
  const clean = DOMPurify.sanitize(html, {
    FORBID_TAGS: ['img', 'style', 'iframe', 'form', 'input', 'button', 'video', 'audio', 'source', 'object', 'embed', 'svg', 'math', 'link', 'meta'],
    FORBID_ATTR: ['style', 'srcset'],
  });
  // Wren's own copy button on every code block, added after sanitizing (the
  // AI's text can't add buttons). Clicks are handled by the chat (copyCode).
  return clean.replace(/<pre>/g, `<div class="codeblock">${COPY_BUTTON}<pre>`).replace(/<\/pre>/g, '</pre></div>');
}

const COPY_ICON =
  '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
export const CHECK_ICON =
  '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5"/></svg>';
const COPY_BUTTON = `<button type="button" class="copybtn" aria-label="Copy" title="Copy">${COPY_ICON}</button>`;
export { COPY_ICON };

/** Put text on the clipboard (with a fallback for webviews without the async API). */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand('copy');
    ta.remove();
    return ok;
  }
}
