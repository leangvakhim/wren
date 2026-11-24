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
  return DOMPurify.sanitize(html, {
    FORBID_TAGS: ['img', 'style', 'iframe', 'form', 'input', 'button', 'video', 'audio', 'source', 'object', 'embed', 'svg', 'math', 'link', 'meta'],
    FORBID_ATTR: ['style', 'srcset'],
  });
}
