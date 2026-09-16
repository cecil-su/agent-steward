import { Marked } from 'marked';
import DOMPurify from 'dompurify';

const escape = (text: string) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
const markdown = new Marked({
  gfm: true,
  breaks: true,
  renderer: {
    // Raw HTML is readable source, never executable markup. Images never fetch resources.
    html: ({ text }) => escape(text),
    image: ({ text }) => escape(text),
    link({ href, title, tokens }) {
      const label = this.parser.parseInline(tokens);
      // Do not turn arbitrary schemes, protocol-relative URLs, or local files into links.
      if (!/^(https?:\/\/|mailto:)/i.test(href)) return label;
      return `<a href="${escape(href)}"${title ? ` title="${escape(title)}"` : ''} rel="noreferrer noopener">${label}</a>`;
    },
  },
});

/** Only use this sanitized result for rich-text DOM insertion. Storage/copy retain the source. */
export function renderMarkdown(source: string): string {
  return DOMPurify.sanitize(markdown.parse(source, { async: false }), {
    ALLOWED_TAGS: ['p', 'br', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'ul', 'ol', 'li', 'blockquote', 'pre', 'code', 'strong', 'em', 'del', 'hr', 'a', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'input'],
    ALLOWED_ATTR: ['href', 'title', 'rel', 'start', 'align', 'type', 'checked', 'disabled'],
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false,
  });
}
