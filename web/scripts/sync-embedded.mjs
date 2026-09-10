// Explicit release step; normal web builds never change the Rust fallback snapshot.
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkDist } from './check-dist.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = path.join(root, 'dist');
const target = path.resolve(root, '../crates/server/web-readonly');
await checkDist(source);
const files = ['index.html', 'app.js', 'style.css'];
const contents = await Promise.all(files.map((file) => fs.readFile(path.join(source, file))));
await fs.mkdir(target, { recursive: true });
for (const entry of await fs.readdir(target)) {
  if (!files.includes(entry)) throw new Error(`Unexpected embedded snapshot file: ${entry}`);
}
for (const [index, file] of files.entries()) await fs.writeFile(path.join(target, file), contents[index]);
await checkDist(target);
console.log('Synced readonly Rust fallback; rebuild taskd before packaging.');
