import { build } from 'vite';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// Keep the native three-file package self-contained; never touch releases or embedded resources.
const root = fileURLToPath(new URL('../', import.meta.url));
const native = new URL('../../crates/server/web-legacy-readonly/', import.meta.url);
const marker = '/* END GENERATED MARKDOWN */\n';
const result = await build({
  configFile: false, root,
  build: {
    write: false, minify: true,
    lib: { entry: `${root}src/lib/markdown.ts`, name: 'StewardMarkdown', formats: ['iife'] },
  },
});
const chunk = (Array.isArray(result) ? result : [result]).flatMap(item => item.output).find(item => item.type === 'chunk');
if (!chunk) throw new Error('Missing Markdown bundle');
for (const [name, generated] of [['app.js', chunk.code], ['style.css', await readFile(new URL('../src/markdown.css', import.meta.url), 'utf8')]]) {
  const path = new URL(name, native);
  const original = await readFile(path, 'utf8');
  const end = original.indexOf(marker);
  const body = end < 0 ? original : original.slice(end + marker.length);
  await writeFile(path, `/* Generated from web/src by npm run build:native-markdown; do not edit this prefix. */\n${generated}\n${marker}${body}`);
}
