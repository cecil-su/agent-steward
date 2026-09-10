import type { TaskContext } from './contracts';

export function formatTaskContext(context: TaskContext): string {
  const { task } = context;
  const json = (value: unknown) => value == null ? '未提供' : JSON.stringify(value, null, 2);
  return [
    `# ${task.title || '未命名任务'} (#${task.id})`,
    `状态：${task.status} · version ${task.version}`,
    '', '## 目标', task.goal || '尚未填写',
    '', '## 范围', task.scope || '尚未填写',
    '', '## 验收', task.acceptanceCriteria || '尚未填写',
    '', '## 下一步', task.nextStep || '尚未填写',
    '', '## 所属项目', context.project ? `${context.project.name} (##${context.project.id})` : '未关联或后端未提供',
    `组件 ID：${(task.componentIds ?? []).join(', ') || '无'}`,
    '', '## 项目资料与来源（长期资料，非实时状态）', json(context.projectProfile),
    '资料引用与版本校验不等于系统验证内容；执行前结合来源任务和依据核实。',
    '', '## Checkpoint', json(context.checkpoint),
    '', '## Checkpoint 后的备注', json(context.notesSinceCheckpoint ?? []),
    ...(context.notesTruncated ? ['备注已截断，请查询完整备注。'] : []),
    '', '## 当前执行会话', json(context.session),
    '', '## 本次代码现场观察', json(context.worktreeStatus),
    '', '未提供现场不代表干净；以上仅为本次查询快照。',
    '继续执行前重新读取最新 Task version、Session 归属及 Git 现场；本文不授权自动领取、修改或关闭任务。',
  ].join('\n');
}
