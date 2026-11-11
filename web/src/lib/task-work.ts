export type RequirementStatus = 'todo' | 'in_progress' | 'blocked' | 'paused' | 'done' | 'cancelled';
export interface Requirement {
  id: string; title: string; acceptanceCriteria: string; status: RequirementStatus;
  result: string; nextStep: string; evidence: string[]; updatedAt: string;
}
export interface Confirmation {
  id: string; requirementIds: string[]; question: string; recommendation: string; impact: string;
  status: 'open' | 'resolved' | 'withdrawn'; answer: string; evidence: string[]; updatedAt: string;
}
export interface TaskWork { formatVersion: 1; requirements: Requirement[]; confirmations: Confirmation[] }
export const requirementLabels: Record<RequirementStatus, string> = {
  todo: '待处理', in_progress: '进行中', blocked: '受阻', paused: '暂缓', done: '已完成', cancelled: '已取消',
};
export function validWork(value: TaskWork): boolean {
  return value.formatVersion === 1 && Array.isArray(value.requirements) && Array.isArray(value.confirmations)
    && value.requirements.every(r => r && typeof r.id === 'string' && typeof r.title === 'string'
      && Object.hasOwn(requirementLabels, r.status) && typeof r.result === 'string'
      && typeof r.acceptanceCriteria === 'string' && typeof r.nextStep === 'string'
      && Array.isArray(r.evidence) && r.evidence.every(e => typeof e === 'string'))
    && value.confirmations.every(c => c && ['open', 'resolved', 'withdrawn'].includes(c.status)
      && typeof c.question === 'string' && typeof c.recommendation === 'string' && typeof c.impact === 'string'
      && typeof c.answer === 'string' && Array.isArray(c.evidence) && c.evidence.every(e => typeof e === 'string')
      && Array.isArray(c.requirementIds) && c.requirementIds.every(id => value.requirements.some(r => r.id === id)));
}
