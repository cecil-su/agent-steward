import { renderMarkdown } from '../lib/markdown';
import '../markdown.css';

export function Markdown({ text }: { text: string }) {
  return <div className="steward-markdown" dangerouslySetInnerHTML={{ __html: renderMarkdown(text) }} />;
}
