import fs from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';
import type { TaskWork } from './task-work';
const source = fs.readFileSync('../crates/server/web-legacy-readonly/app.js', 'utf8');
const html = fs.readFileSync('../crates/server/web-legacy-readonly/index.html', 'utf8');
afterEach(() => { document.body.replaceChildren(); vi.restoreAllMocks(); });
function fixture() {
  document.body.innerHTML = new DOMParser().parseFromString(html, 'text/html').body.innerHTML;
  const fetch = vi.fn(); const end = source.lastIndexOf('  startUiUpdates();');
  const code = source.slice(0, end).replace('(() => {', 'return (() => {') + 'return {workContent};})();';
  const api = new Function('document', 'fetch', 'navigator', code)(document, fetch, {});
  const root = document.createElement('div'); document.body.append(root);
  return { ...api, root, fetch };
}
const snapshot = (): TaskWork => ({ formatVersion: 1, requirements: [{ id: 'R1', title: '预览尺寸', acceptanceCriteria: '300px', status: 'done', result: '300px已生效', nextStep: '', evidence: ['核对请求'], updatedAt: '2026-10-29' }], confirmations: [{ id: 'Q1', requirementIds: ['R1'], question: '业务选项', recommendation: '选A', impact: '决定处理范围', status: 'open', answer: '', evidence: [], updatedAt: '2026-10-29' }] });
it('shows persisted statuses/results and decisions without business write controls or network', () => {
  const f = fixture(); f.workContent(f.root, snapshot());
  expect(f.root).toHaveTextContent('300px已生效'); expect(f.root).toHaveTextContent('已完成');
  expect(f.root.querySelector('[aria-label="待你确认"]')).toHaveTextContent('业务选项');
  expect(f.root.querySelector('button,input,select,textarea')).toBeNull(); expect(f.fetch).not.toHaveBeenCalled();
});
it('resolved questions leave the pending section but retain answer/evidence in disclosure', () => {
  const f = fixture(); const work = snapshot(); work.confirmations[0] = { ...work.confirmations[0], status: 'resolved', answer: '用户选择A', evidence: ['明确答复'] };
  f.workContent(f.root, work);
  expect(f.root.querySelector('[aria-label="待你确认"]')).toHaveTextContent('暂无需要你确认');
  expect(f.root.querySelector('[aria-label="待你确认"]')).not.toHaveTextContent('业务选项');
  expect(f.root.querySelector('[data-detail-key="work-decisions-history"]')).toHaveTextContent('用户选择A');
});
it('missing/unsupported state is not an empty list; contents remain inert', () => {
  const f = fixture();
  for (const [work, text] of [[undefined,'尚未接入'],[null,'尚未整理'],[{ formatVersion: 2 },'格式不可用']] as const) {
    f.root.replaceChildren(); f.workContent(f.root, work); expect(f.root).toHaveTextContent(text);
  }
  const work = snapshot(); work.requirements[0].result='<img src=x onerror=alert(1)>\n![blocked](https://example.invalid/x.png)';
  f.root.replaceChildren(); f.workContent(f.root,work);
  expect(f.root.querySelector('img,script')).toBeNull();
});
