import { render, screen } from '@testing-library/react';
import { expect, it } from 'vitest';
import { TaskWorkPanel } from './task-work-panel';
import type { TaskWork } from '../lib/task-work';
const snapshot = (): TaskWork => ({ formatVersion: 1, requirements: [
  { id: 'R1', title: '商品图片', acceptanceCriteria: '使用300px预览', status: 'done', result: '300px已生效', nextStep: '', evidence: ['已核对请求'], updatedAt: '2026-10-29' },
  { id: 'R2', title: '数量同步', acceptanceCriteria: '删除后同步', status: 'in_progress', result: '', nextStep: '核对分页删除', evidence: [], updatedAt: '2026-10-29' },
], confirmations: [{ id: 'Q1', requirementIds: ['R2'], question: '选A还是B？', recommendation: 'A', impact: '影响统计口径', status: 'open', answer: '', evidence: [], updatedAt: '2026-10-29' }] });
it('distinguishes unavailable, unorganized and unsupported work instead of showing false completion', () => {
  const { rerender } = render(<TaskWorkPanel />);
  expect(screen.getByText(/尚未接入/)).toBeVisible();
  rerender(<TaskWorkPanel work={null} />); expect(screen.getByText(/尚未整理/)).toBeVisible();
  rerender(<TaskWorkPanel work={{ ...snapshot(), formatVersion: 2 } as unknown as TaskWork} />);
  expect(screen.getByRole('status')).toHaveTextContent('格式不可用');
});
it('shows requirement results/statuses and only open decisions, preserving resolved history without write controls', () => {
  const work = snapshot(); const { rerender } = render(<TaskWorkPanel work={work} />);
  expect(screen.getByText('300px已生效')).toBeVisible();
  expect(screen.getByText('已完成')).toBeVisible(); expect(screen.getByText('进行中')).toBeVisible();
  expect(screen.getByRole('region', { name: '待你确认' })).toHaveTextContent('选A还是B？');
  expect(screen.queryByRole('button')).toBeNull(); expect(screen.queryByRole('textbox')).toBeNull();
  work.confirmations[0] = { ...work.confirmations[0], status: 'resolved', answer: '用户选择A', evidence: ['本轮答复'] };
  rerender(<TaskWorkPanel work={work} />);
  expect(screen.getByRole('region', { name: '待你确认' })).not.toHaveTextContent('选A还是B？');
  expect(screen.getByText('暂无需要你确认的事项。')).toBeVisible();
  expect(screen.getByText('用户选择A').closest('details')).toHaveTextContent('已处理的确认事项（1）');
});
it('renders untrusted requirement/decision contents as safe text or sanitized Markdown', () => {
  const work = snapshot(); work.requirements[0].title = '<img src=x onerror=alert(1)>';
  work.requirements[0].result = '<script>alert(1)</script>\n![external](https://example.invalid/x.png)';
  const { container } = render(<TaskWorkPanel work={work} />);
  expect(container.querySelector('img,script')).toBeNull(); expect(container).toHaveTextContent('<img src=x onerror=alert(1)>');
});
