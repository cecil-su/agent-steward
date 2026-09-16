import { render, screen } from '@testing-library/react';
import { expect, it } from 'vitest';
import { Markdown } from './markdown';
import { TaskDetailPanel } from '../features/detail-panels';
import type { TaskContext } from '../lib/contracts';

it('renders headings, lists, quotes, code, tables, safe links and plain line breaks', () => {
  const { container } = render(<Markdown text={'# 标题\n\n第一行\n第二行\n\n- **重点**\n- [x] 完成\n\n> 引用\n\n```js\nconst x = "<tag>";\n```\n\n| 列 | 值 |\n| --- | --- |\n| 内容 | `code` |\n\n[文档](https://example.com)'} />);
  expect(screen.getByRole('heading', { name: '标题' })).toBeInTheDocument();
  expect(container.querySelector('strong')).toHaveTextContent('重点');
  expect(container.querySelector('blockquote')).toHaveTextContent('引用');
  expect(container.querySelector('pre code')).toHaveTextContent('const x = "<tag>";');
  expect(container.querySelector('table td')).toHaveTextContent('内容');
  expect(container.querySelector('br')).not.toBeNull();
  expect(screen.getByRole('checkbox')).toBeDisabled();
  expect(screen.getByRole('link')).toHaveAttribute('href', 'https://example.com');
});

it('keeps HTML visible but inert and never loads images or unsafe links', () => {
  const { container } = render(<Markdown text={'<script>alert(1)</script>\n<img src=x onerror=alert(1)>\n<svg onload=alert(1)></svg>\n\n![external](https://example.com/image)\n\n[bad](javascript:alert%281%29) [data](data:text/html,x) [file](file:///C:/secret) [relative](//example.com)\n\n[encoded](jav&#x61;script:alert%281%29)\n\n**safe**'} />);
  expect(container.querySelector('script,img,svg,iframe,style')).toBeNull();
  expect(container.querySelector('a')).toBeNull();
  expect(container).toHaveTextContent('<script>alert(1)</script>');
  expect(container.querySelector('strong')).toHaveTextContent('safe');
});

it('uses the same renderer for full notes, recent notes and every checkpoint text field', () => {
  const note = { id: 1, text: '**备注**', noteType: 'progress', createdAt: '' };
  const context = { task: {}, notesSinceCheckpoint: [note], checkpoint: { summary: '**摘要**', completed: ['**完成**'], decisions: ['**决策**'], pending: ['**待办**'], risks: ['**风险**'], nextStep: '**下一步**' } } as unknown as TaskContext;
  const { container, rerender } = render(<TaskDetailPanel tab="overview" context={context} />);
  expect(Array.from(container.querySelectorAll('strong'), node => node.textContent)).toEqual(['摘要', '完成', '决策', '待办', '风险', '下一步', '备注']);
  rerender(<TaskDetailPanel tab="notes" context={context} notes={[note]} />);
  expect(container.querySelector('strong')).toHaveTextContent('备注');
  rerender(<TaskDetailPanel tab="notes" context={context} notes={[]} />);
  expect(screen.getByText('暂无进展备注')).toBeInTheDocument();
  rerender(<TaskDetailPanel tab="notes" context={context} />);
  expect(screen.getByText('进展备注尚未提供')).toBeInTheDocument();
});
