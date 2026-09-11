# 项目资料与 CLI 维护

## 范围

项目资料使用Project身份、revision CAS和project_history。Web只读展示资料与来源；写入由CLI/Application提供，没有项目资料写HTTP入口。当前数据库Schema7、UI合同4、包格式1。

## 数据合同

`project_profiles`每项目最多一条当前资料；维护事件为`project.profile_updated`。

| 字段 | 约束 |
| --- | --- |
| summary | 简介，trim后1–4000 Unicode字符 |
| architecture | 架构与入口，1–8000字符 |
| development | 开发与验证方法，1–8000字符 |
| evidence | 核实依据，1–4000字符 |
| sourceTaskId | 正整数，写入时必须是该项目关联任务 |
| sourceTaskVersion | 正整数，必须等于写入时任务当前version |

所有字段必填、不接受null/未知字段；文本拒绝NUL。CLI文件/stdin输入最多512 KiB，读取阶段限流；JSON解析错误不回显非法值。

返回资料包含`projectId/revision/updatedAt`。资料revision是最近一次资料维护时的Project revision，不是独立计数器；项目其他维护后当前Project revision可更高。

更新是全量替换，不是Patch；不能省略字段表示保留，不能用空串清除。相同内容再次显式提交也作为一次维护。一个写事务内检查项目revision、来源任务归属与version，写当前资料、递增Project revision并保存完整before/after及来源。失败全部回滚，不修改来源Task、Task History或Session。

sourceTaskVersion是写入时快照，任务后续推进不会改写资料来源。引用一致性不证明正文真实，不构成自动验真或操作授权。

## 隔离使用

先构建并核对项目内CLI。示例只使用获准的隔离库；编号/revision/version取自刚读取的返回值。

```powershell
$cli = '.\target\debug\taskctl.exe'
$db = 'E:\sandbox\steward-profile\state.db'

& $cli --database $db --json project show '##1'
& $cli --database $db --json task context 1
& $cli --database $db --json project profile show '##1'
& $cli --database $db --json project profile set '##1' --if-revision REVISION --input profile.json
& $cli --database $db --json project history '##1' --after 0 --limit 50
```

`profile.json`示例；来源任务必须在该隔离库中存在并属于目标项目：

```json
{
  "summary": "经核实的项目用途与边界",
  "architecture": "主要模块、入口及调用关系",
  "development": "开发工具链与隔离验证命令",
  "sourceTaskId": 1,
  "sourceTaskVersion": 7,
  "evidence": "核实依据及相关源码/文档路径，说明未确认边界"
}
```

先读取当前资料、Project revision与来源任务上下文，再核实内容。冲突后重新读取并判断，不自动填入新revision重试。长期资料描述用途、架构、入口与验证方法，不固化某次测试通过、工作树clean或服务运行等易失状态。不得保存凭据或隐藏推理，没有自动秘密过滤保证。

## 读取与上下文

- `project profile show`返回`{project,profile}`，无资料时`profile:null`。
- `project show`及`GET /api/projects/{id}`返回profile。
- `task context`及`GET /api/tasks/{id}/context`在同一数据库读事务中带projectProfile。
- 项目History按revision分页，保留来源和before/after。
- 工作台项目详情、任务概览及复制上下文展示资料与来源；缺字段与明确null分开处理。
- 项目资料不是实时源码观察，不混入源码导航指纹。

## 导入与验证

Schema4快照可通过`database import-schema4`复制到Schema7，目标项目资料和规则表为空；Schema5/6输入包含资料并按原字节保留。准确入口及路径、停写、发布与恢复限制见[隔离导入与验证](18-Schema7隔离导入与验证.md)。不能把输入Schema值改为目标版本。

验证入口为Application/CLI的`project_profiles.rs`测试、`crates/server/tests/projects.rs`及`web/src/features/workspace.test.tsx`。覆盖资料CAS、来源归属/version、History回滚、输入限额和只读展示；仅使用临时合成数据。真实目标实例、跨平台、人工页面及业务验收须独立验证，不从测试文件或源码推断正式状态。
