import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { SessionRulesPanel } from './detail-panels';
import { formatTaskContext } from '../lib/context';
import type { SessionRules, Task } from '../lib/contracts';
const rules: SessionRules = { formatVersion: 1, rules: [{ id: 4, revision: 3, scope: 'global', projectId: null, contentVersion: 1, content: { name: '自由偏好', body: '<script>不执行</script>\n完整正文', sources: [{ kind: 'inferred', evidence: '两个任务反馈', taskId: 1, taskVersion: 2 }] } }] };
describe('effective session rules', () => {
  it('distinguishes missing contract from an empty effective set', () => {
    const { rerender } = render(<SessionRulesPanel />);
    expect(screen.getByRole('status')).toHaveTextContent('规则不可用');
    rerender(<SessionRulesPanel rules={{ formatVersion: 1, rules: [] }} />);
    expect(screen.getByText('暂无有效规则。')).toBeVisible();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });
  it('renders full escaped content and historical evidence without write controls', () => {
    const { container } = render(<SessionRulesPanel rules={rules} />);
    expect(screen.getByText(/完整正文/)).toBeVisible();
    expect(screen.getByText(/历史版本 2/)).toBeVisible();
    expect(container.querySelector('script')).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
    expect(screen.queryByRole('textbox')).toBeNull();
  });
  it('copies complete rules into the same task context handoff', () => {
    const task: Task = { id: 2, version: 1, title: null, status: 'open', goal: null, scope: null, acceptanceCriteria: null, nextStep: null };
    const text = formatTaskContext({ task, sessionRules: rules });
    expect(text).toContain('完整正文'); expect(text).toContain('两个任务反馈'); expect(text).toContain('"revision": 3');
  });
});
