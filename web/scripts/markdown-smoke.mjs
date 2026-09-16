import { chromium } from 'playwright';
import { readFile, mkdir } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

// Static synthetic fixture only: no server, business database, or installed UI is accessed.
const native = new URL('../../crates/server/web-legacy-readonly/', import.meta.url);
const source = await readFile(new URL('app.js', native), 'utf8');
const css = await readFile(new URL('style.css', native), 'utf8');
const html = (await readFile(new URL('index.html', native), 'utf8')).replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '').replace(/<link\b[^>]*>/gi, '');
const artifacts = new URL(`../.artifacts/markdown-${new Date().toISOString().replace(/[:.]/g, '-')}/`, import.meta.url);
await mkdir(artifacts, { recursive: true });
const browser = await chromium.launch({ channel: process.env.STEWARD_BROWSER_CHANNEL || 'chrome', headless: true });
try {
  const page = await browser.newPage();
  const errors = [], requests = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => { requests.push(route.request().url()); return route.abort(); });
  await page.setContent(html);
  await page.addStyleTag({ content: css });
  const text = '# 阅读标题\n\n第一行\n第二行\n\n- **重点**\n- [x] 已完成\n\n> 引用说明\n\n```text\n' + 'long_code_'.repeat(80) + '\n```\n\n| 列 | 值 |\n| --- | --- |\n| 内容 | `code` |\n\n[文档](https://example.com)\n\n<img src=x onerror=alert(1)>\n\n![外部图片](https://example.com/image)';
  await page.evaluate(text => {
    window.fixtureText = text;
    window.fetch = async (url, options) => {
      if (options?.method && options.method !== 'GET') throw new Error('Unexpected write');
      if (!String(url).endsWith('/notes')) throw new Error(`Unexpected API ${url}`);
      return new Response(JSON.stringify({ ok: true, data: { notes: [{ id: 1, noteType: 'progress', text, createdAt: '' }] } }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    };
  }, text);
  const end = source.lastIndexOf('  startUiUpdates();');
  assert(end > 0);
  await page.addScriptTag({ content: source.slice(0, end) + `
    connected=true;selected=1;context={task:{id:1,title:'Markdown 合成阅读检查',status:'done',closureOutcome:'completed',version:3,goal:window.fixtureText,scope:'**范围**',acceptanceCriteria:'- 验收条目'},sessionRules:{formatVersion:1,rules:[]},checkpoint:{summary:'**摘要**',completed:['**完成项**'],decisions:[],pending:[],risks:[],nextStep:'**下一步**'}};
    $('login').hidden=true;$('workspace').hidden=false;
    window.fixtureRendered=renderDetail();
  })();` });
  await page.evaluate(() => window.fixtureRendered);
  for (const width of [1360, 390, 320]) {
    await page.setViewportSize({ width, height: 1000 });
    assert(await page.locator('.steward-markdown table').count() >= 2);
    assert.equal(await page.locator('.steward-markdown img,.steward-markdown script').count(), 0);
    const layout = await page.evaluate(() => ({
      viewport: innerWidth, scroll: document.documentElement.scrollWidth,
      pre: [...document.querySelectorAll('.steward-markdown pre')].map(node => ({ whitespace: getComputedStyle(node).whiteSpace, overflow: getComputedStyle(node).overflowX, width: node.clientWidth, scroll: node.scrollWidth })),
    }));
    assert(layout.scroll <= layout.viewport, JSON.stringify(layout));
    assert(layout.pre.every(pre => pre.whitespace === 'pre' && pre.overflow === 'auto' && pre.scroll > pre.width), JSON.stringify(layout));
    await page.screenshot({ path: fileURLToPath(new URL(`${width}.png`, artifacts)), fullPage: true });
  }
  assert.deepEqual(errors, []);
  assert.deepEqual(requests, []);
  console.log(`PASS: Chrome ${browser.version()}, 1360/390/320px; no network or writes; ${artifacts.pathname}`);
} finally {
  await browser.close();
}
