const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../../web-legacy-readonly/app.js'), 'utf8');
// Exercise the actual readonly renderer without a server, credentials, or database.
const start = source.indexOf('  function taskHistoryItem('), end = source.indexOf('  async function loadEvents(', start);
assert(start >= 0 && end > start);
const el = (tag, text = '') => ({ tag, text, dataset: {}, children: [], append(...items) { this.children.push(...items); } });
const section = (title, value) => { const node = el('section'); node.append(el('h3', title), el('p', Array.isArray(value) ? value.join('\n') : value)); return node; };
const render = vm.runInNewContext(source.slice(start, end) + '\ntaskHistoryItem', { el, section, date: value => value, statuses: { open: '待开始', closed: '已关闭' } });
const visible = node => String(node.text ?? '') + (node.tag === 'details' && !node.open ? node.children.slice(0, 1) : node.children).map(visible).join('\n');
const all = node => String(node.text ?? '') + node.children.map(all).join('\n');
const entry = (type, payload) => ({ sequence: 12, occurredAt: '2026-09-09T10:00:00Z', changeType: type, payload });

test('checkpoint IDs are hidden in technical details; old checkpoint never borrows current content', () => {
  const row = render(entry('checkpoint.saved', { checkpointId: 'old-private-id', sessionId: 'session-private-id', gitHead: null }), { checkpoint: { id: 'latest-id', summary: 'Not this historical summary' } }, [], false, new Set());
  assert(visible(row).includes('保存进展检查点'));
  assert(visible(row).includes('未提供这份历史正文'));
  assert(!visible(row).includes('old-private-id'));
  assert(!visible(row).includes('Not this historical summary'));
  assert(!visible(row).includes('gitHead'));
  assert(all(row).includes('old-private-id'));
});

test('exact checkpoint match exposes its summary and expandable progress content', () => {
  const row = render(entry('checkpoint.saved', { checkpointId: 'cp-1' }), { checkpoint: { id: 'cp-1', summary: '真实匹配的摘要', completed: ['完成项'], nextStep: '下一步内容' } }, [], false, new Set(['task-checkpoint-12']));
  assert(visible(row).includes('真实匹配的摘要'));
  assert(visible(row).includes('完成项'));
  assert(visible(row).includes('下一步内容'));
});

test('maintenance presents translated changed fields and does not invent missing before values', () => {
  const row = render(entry('task.updated', { before: { title: '旧标题', projectId: null, componentIds: [], version: 1 }, after: { title: '新标题', projectId: 1, componentIds: [2], version: 2 }, reason: '用户确认' }), {}, [], false, new Set(['task-history-changes-12']));
  assert(visible(row).includes('本次变更 3 项'));
  for (const text of ['修改前：旧标题', '修改后：新标题', '未关联项目', '项目 ##1', '未限定组件', '组件 #2', '用户确认']) assert(visible(row).includes(text));
  assert(!visible(row).includes('version'));
  const old = render(entry('task.updated', { goal: '旧版记录的新目标' }), {}, [], false, new Set(['task-history-changes-12']));
  assert(visible(old).includes('修改前：历史记录未保存'));
});

test('note text uses exact ID and failures remain explicit', () => {
  const h = entry('task.noted', { noteId: 8, noteType: 'risk' });
  assert(visible(render(h, {}, [{ id: 8, noteType: 'risk', text: '实际风险内容' }], false, new Set())).includes('实际风险内容'));
  assert(!visible(render(h, {}, [{ id: 9, text: '另一条备注' }], false, new Set())).includes('另一条备注'));
  assert(visible(render(h, {}, null, true, new Set())).includes('正文读取失败'));
});

test('closure is readable and raw unknown events remain accessible', () => {
  const row = render(entry('task.closed', { outcome: 'cancelled', reason: '范围取消', closedSessionId: 'internal-session' }), {}, [], false, new Set());
  assert(visible(row).includes('取消')); assert(visible(row).includes('范围取消')); assert(!visible(row).includes('internal-session'));
  const unknown = render({ ...entry('future.event', { field: 'original data' }), summary: '原事件说明' }, {}, [], false, new Set(['task-history-technical-12']));
  assert(visible(unknown).includes('原事件说明')); assert(visible(unknown).includes('original data'));
});
