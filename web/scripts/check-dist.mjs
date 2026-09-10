import { readdir, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parse as parseHtml } from 'parse5';
import { init, parse as parseModules } from 'es-module-lexer';

// Package-format-1 resource gate, not permission to activate an incomplete UI.
export async function checkDist(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const names = entries.map((entry) => entry.name).sort();
  if (names.join(',') !== 'app.js,index.html,style.css' || entries.some((entry) => !entry.isFile())) {
    throw new Error('UI must contain exactly three regular files: index.html, app.js, style.css');
  }
  const files = {};
  for (const name of names) {
    const bytes = await readFile(resolve(directory, name));
    if (bytes.length === 0 || bytes.length > 4 * 1024 * 1024) throw new Error(`Invalid resource size: ${name}`);
    files[name] = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  }
  const html = files['index.html'];
  if (!html.includes('<head>') || !html.includes('src="/app.js"') || !html.includes('href="/style.css"')) {
    throw new Error('HTML is missing package-format-1 entry markers');
  }
  let scriptCount = 0;
  let stylesheetCount = 0;
  function visit(node) {
    const attributes = Object.fromEntries((node.attrs ?? []).map(({ name, value }) => [name, value]));
    if (['style', 'base', 'iframe', 'object', 'embed'].includes(node.tagName)) throw new Error('Unexpected HTML resource element');
    for (const [name, value] of Object.entries(attributes)) {
      if (name === 'style' || name.startsWith('on')) throw new Error('Inline styles/handlers are not allowed');
      if (['srcset', 'imagesrcset', 'poster', 'data', 'srcdoc', 'background', 'xlink:href'].includes(name)) throw new Error('Unexpected HTML resource attribute');
      if (name === 'src' && !(node.tagName === 'script' && value === '/app.js')) throw new Error('Unexpected HTML resource');
      if (name === 'href' && !(node.tagName === 'link' && value === '/style.css')) throw new Error('Unexpected HTML resource');
    }
    if (node.tagName === 'script') {
      scriptCount++;
      if (attributes.src !== '/app.js' || node.childNodes?.some((child) => child.value?.trim())) throw new Error('Only the external app.js entry is allowed');
    }
    if (node.tagName === 'link') {
      if (attributes.href !== '/style.css' || attributes.rel !== 'stylesheet') throw new Error('Unexpected HTML resource');
      stylesheetCount++;
    }
    for (const child of node.childNodes ?? []) visit(child);
    if (node.content) visit(node.content);
  }
  visit(parseHtml(html));
  if (scriptCount !== 1 || stylesheetCount !== 1) throw new Error('Expected exactly one script and one stylesheet');
  await init;
  const [imports] = parseModules(files['app.js']);
  if (imports.some((entry) => entry.d !== -2)) throw new Error('JavaScript must not import additional modules');
  // Conservative check for ordinary worker constructors. This is not a JS sandbox
  // (aliases/computed property access must still be caught in code review).
  if (/\bnew\s+(?:(?:window|globalThis|self)\s*\.\s*)?(?:Worker|SharedWorker)\s*\(/.test(files['app.js'])) {
    throw new Error('Workers are not supported by UI package format 1');
  }
  if (/@import\b|url\s*\(/i.test(files['style.css'])) throw new Error('CSS must not load additional resources');
  return names;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  await checkDist(resolve('dist'));
  console.log('PASS: three-file UI resources, UTF-8, size limits and entry markers');
}
