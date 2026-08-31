# TUI 与 GUI 协同交互流程图

本图以“从任一客户端创建任务，在另一客户端观察或审批”为例，展示 TUI、GUI、任务角色和外层优化角色如何通过同一状态权威协同。

```mermaid
sequenceDiagram
    actor User as 用户
    participant TUI as TUI
    participant GUI as GUI
    participant Core as taskd
    participant Manager as Task Manager
    participant Owner as Task Owner
    participant Agents as Subagents
    participant Steward as Context & Optimization Steward

    alt 从 TUI 发起
        User->>TUI: 创建或更新任务
        TUI->>Core: Command + expectedVersion
    else 从 GUI 发起
        User->>GUI: 创建或更新任务
        GUI->>Core: Command + expectedVersion
    end

    Core->>Core: 校验、持久化并产生事件
    Core-->>TUI: 推送最新任务状态
    Core-->>GUI: 推送最新任务状态

    Core->>Manager: 登记任务并计算下一步
    Manager->>Owner: 指定任务 Owner 与验收条件
    Owner->>Core: 请求相关需求、偏好和历史决策
    Core-->>Owner: 返回精简 Context Brief

    Owner->>Agents: 拆分并派发 Assignments

    loop 任务推进
        Agents-->>Owner: 进度、阻塞、产物和测试证据
        Owner->>Core: 更新任务进度与执行证据
        Core-->>TUI: 实时状态、日志和阻塞提醒
        Core-->>GUI: 看板、进度和产物更新

        alt 任务被阻塞
            User->>TUI: 补充要求或处理阻塞
            TUI->>Core: 记录决策与任务级上下文
            Core-->>GUI: 同步阻塞处理结果
        else 需要可视化审批
            User->>GUI: 审阅计划、差异或产物
            GUI->>Core: 确认、拒绝或要求返工
            Core-->>TUI: 同步审批结果
        end
    end

    Owner->>Core: 提交完成候选与验收证据
    Core-->>GUI: 展示结果、测试和差异
    Core-->>TUI: 显示完成候选与检查摘要

    alt 用户验收通过
        User->>GUI: 确认任务完成
        GUI->>Core: Accept Task
        Core->>Manager: 更新任务状态和下一步
    else 用户要求返工
        User->>TUI: 说明问题和修正要求
        TUI->>Core: Reopen / Request Changes
        Core->>Owner: 重新进入任务循环
    end

    Core-->>Steward: 提供反馈、返工、澄清和执行指标
    Steward->>Core: 提交需求、偏好、Prompt 或流程候选
    Core-->>GUI: 展示候选证据与影响
    Core-->>TUI: 提示存在待确认候选

    alt 用户确认长期生效
        User->>GUI: 批准候选
        GUI->>Core: Promote Proposal
        Core->>Core: 更新需求、偏好或模板版本
        Core-->>TUI: 同步新版本
    else 拒绝或仅本次有效
        User->>TUI: 拒绝或标记为任务级上下文
        TUI->>Core: Reject / Keep Task-local
        Core-->>GUI: 同步处理结果
    end
```

## 推荐的客户端分工

| 场景 | TUI | GUI |
|---|---|---|
| 快速创建、查询和推进任务 | 主入口 | 支持 |
| 执行日志和实时状态 | 流式查看 | 聚合查看 |
| 任务看板和依赖关系 | 简化列表 | 主入口 |
| 项目需求与偏好维护 | 快速编辑 | 主入口 |
| 计划、差异和产物审批 | 可信终端确认 | 可视化主入口 |
| 优化候选审阅 | 摘要与命令 | 证据、对比和影响分析 |
