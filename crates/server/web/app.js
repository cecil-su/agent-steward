'use strict';
(() => {
  const $ = id => document.getElementById(id);
  const statuses = {open:'待开始',in_progress:'进行中',blocked:'有阻塞',closed:'已关闭'};
  let warningText='', listRevision=0;
  let projectsSupported=false,projectPageVisible=false,projectFilter='',projectFilterRevision=0,projectRows=[],projectCursor=null,projectListRevision=0,projectDetailRevision=0,projectContext=null,selectedProject=null;
  let pendingWrites=0, uncertainWrite=false, modalEpoch=0;
  let canWrite=false,liveAbort=null,liveTimer=null,liveDirty=false,liveRefreshing=false;
  let localAccess=false, connected=false, view='active', query='', cursor=null, rows=[], selected=null, context=null, activeTab='overview', revision=0, modal=null;
  function el(tag,text,cls) { const node=document.createElement(tag); if(text!==undefined)node.textContent=text; if(cls)node.className=cls; return node; }
  function button(text,action,cls='') { const b=el('button',text,cls);b.type='button';b.onclick=action;return b; }
  function clear(node){node.replaceChildren();}
  function notify(text,success=false){if(success&&warningText){text=warningText+'\n'+text;success=false;}$('notice').textContent=text;$('notice').className='notice'+(success?' success':'');$('notice').hidden=!text;}
  function showWarnings(warnings){if(warnings?.length){warningText=warnings.map(w=>`${w.code}：${w.message}`).join('\n');notify(warningText);}}
  function date(value){return value?new Date(value).toLocaleString('zh-CN',{month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit'}):'—';}
  function badge(status){return el('span',statuses[status]||status,'badge '+status);}
  // Remove the previous JS-readable credential; browser grants now use HttpOnly cookies.
  try{sessionStorage.removeItem('steward.connection-token');}catch{}
  function sessionId(){const bytes=crypto.getRandomValues(new Uint8Array(16));return 'session-'+Array.from(bytes,b=>b.toString(16).padStart(2,'0')).join('');}
  function resetConnection(){stopLive();projectsSupported=false;projectFilter='';projectFilterRevision++;projectListRevision++;projectDetailRevision++;projectRows=[];projectCursor=null;selectedProject=null;projectContext=null;projectPageVisible=false;$('project-filter').value='';$('projects').hidden=true;$('project-filter-form').hidden=true;$('project-page').hidden=true;$('task-page').hidden=false;clear($('project-list'));clear($('project-detail'));canWrite=false;connected=false;warningText='';listRevision++;rows=[];selected=null;context=null;revision++;$('workspace').hidden=true;$('login').hidden=false;$('credential').value='';clear($('task-list'));clear($('detail'));$('action-dialog').close();modal=null;}
  async function api(path,body,extraHeaders={}) {
    if(body!==undefined)pendingWrites++;
    try{return await apiRequest(path,body,extraHeaders);}
    catch(error){if(error.uncertain)uncertainWrite=true;throw error;}
    finally{if(body!==undefined)pendingWrites--;}
  }
  async function apiRequest(path,body,extraHeaders={}) {
    let response;
    try { response=await fetch(path,{method:body===undefined?'GET':'POST',credentials:'same-origin',headers:{'X-Steward-UI-Contract':'1',...(body===undefined?{}:{'Content-Type':'application/json','X-Steward-CSRF':'1'}),...extraHeaders},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(15000),cache:'no-store'}); }
    catch {const error=new Error(body===undefined?'无法读取本地服务，请检查服务是否仍在运行。':'结果未确认：请求中断或超时。请刷新任务和现场，核对是否已执行；不要直接重复提交。');error.uncertain=body!==undefined;throw error;}
    let result;try{result=await response.json();}catch{const error=new Error('服务返回无法识别的结果，请刷新核对。');error.uncertain=body!==undefined;throw error;}
    showWarnings(result.warnings);
    if(!response.ok||!result.ok){const error=new Error(`${result.error?.code||response.status}：${result.error?.message||'请求失败'}`);error.code=result.error?.code;error.details=result.error?.details;error.uncertain=body!==undefined&&(!result.error||['PARTIAL_EXTERNAL_STATE','INTERNAL_ERROR'].includes(error.code));if(response.status===401){resetConnection();$('login-error').textContent='浏览器授权已失效，请重新连接。';}throw error;}
    return result.data;
  }
  function safely(action){return async()=>{try{await action();}catch(e){notify(e.message);}};}
  async function loadList(append=false,preserveRange=false){
    const target=preserveRange?Math.max(30,rows.length):30;
    const generation=++listRevision;const search=new URLSearchParams(view==='closed'?{status:'closed',pageSize:'30'}:{view,pageSize:'30'});if(query)search.set('query',query);if(projectFilter)search.set('project',projectFilter);if(append&&cursor)search.set('cursor',cursor);
    const refreshed=[];let data;
    do{
      data=await api('/api/tasks?'+search);if(generation!==listRevision)return false;
      if(preserveRange&&modal){liveDirty=true;return false;}
      refreshed.push(...data.tasks);
      if(data.hasMore)search.set('cursor',data.nextCursor);
    }while(preserveRange&&data.hasMore&&refreshed.length<target);
    // Publish the complete refreshed range once; failures leave the visible pages and cursor intact.
    rows=append?rows.concat(refreshed):refreshed;cursor=data.nextCursor;
    renderList();$('more').hidden=!data.hasMore;
    return true;
  }
  function renderList(){const list=$('task-list'),panel=list.parentElement,page=document.scrollingElement;
    const panelTop=panel.scrollTop,pageTop=page.scrollTop,items=[];$('task-count').textContent=rows.length+' 项';
    if(!rows.length){const empty=el('div',undefined,'empty');empty.append(el('h2','这里还没有任务'),el('p','新建任务，或切换视图和搜索条件。'));items.push(empty);}
    for(const task of rows){const b=button('',safely(()=>selectTask(task.id)),'task-card'+(selected===task.id?' selected':''));b.setAttribute('aria-current',String(selected===task.id));const meta=el('div',undefined,'card-meta');meta.append(el('span','#'+task.id),badge(task.status));b.append(meta,el('h3',task.title||'未命名任务'),el('p',task.nextStep||task.goal||'等待补充目标和下一步'));items.push(b);}
    list.replaceChildren(...items);panel.scrollTop=panelTop;page.scrollTop=pageTop;
  }
  async function selectTask(id){selected=id;const generation=++revision;renderList();const data=await api(`/api/tasks/${id}/context`);if(generation!==revision)return;context=data;await renderDetail();}
  function section(title,value){const node=el('section',undefined,'section');node.append(el('h3',title));if(Array.isArray(value)){const ul=el('ul');for(const line of value)ul.append(el('li',line));node.append(value.length?ul:el('p','暂无记录','muted'));}else node.append(el('p',value||'尚未填写',value?'':'muted'));return node;}
  function action(text,name,fields,description='',extra={}){const b=button(text,()=>openAction(name,text,fields,description,extra));b.hidden=!canWrite;return b;}
  const text=(key,label,value='',required=true)=>({key,label,value:value??'',required});
  const area=(key,label,value='',required=true)=>({...text(key,label,value,required),type:'textarea'});
  const choose=(key,label,options,value)=>({key,label,type:'select',options,value:value??options[0][0],required:true});
  const check=(key,label)=>({key,label,type:'checkbox',required:true});
  const taskFields=task=>[text('title','标题（MMDD｜类型｜主题）',task?.title,false),area('goal','目标',task?.goal,false),area('scope','范围',task?.scope,false),area('acceptanceCriteria','验收条件',task?.acceptanceCriteria,false),area('nextStep','下一步',task?.nextStep,false)];
  async function renderDetail(){
    if(!context)return;const t=context.task;const root=$('detail');clear(root);
    const heading=el('div',undefined,'detail-heading');const meta=el('div',undefined,'meta');meta.append(el('span','#'+t.id),badge(t.status),el('span','版本 '+t.version),el('span','更新 '+date(t.updatedAt)));heading.append(meta,el('h2',t.title||'未命名任务'));
    const actions=el('div',undefined,'actions');actions.append(button('复制交接上下文',safely(copyContext)));
    if(t.status!=='closed'){
      if(projectsSupported){actions.append(action('关联项目','task-project',[text('project','项目（##编号或唯一名称；留空解除）',t.projectId?'##'+t.projectId:'',false),check('confirmed','确认变更项目归属；跨项目或解除时清空组件范围')],'只改变任务归属，不领取任务、不改变 Session 或 Worktree。'));if(t.projectId){const components=button('设置组件范围',safely(()=>editTaskComponents(t)));components.hidden=!canWrite;actions.append(components);}}
      actions.append(action('编辑任务','task-update',taskFields(t),'只提交修改过的字段。若版本发生冲突，填写内容会保留。'));
      if(t.status==='open')actions.append(action('开始任务','task-claim',[text('sessionId','新执行 Session ID',sessionId())],'显式领取任务并记录当前执行会话。'));
      if(t.status==='in_progress'||t.status==='blocked')actions.append(action('换会话继续','task-resume',[text('sessionId','新执行 Session ID',sessionId()),text('fromSession','来源 Session ID',t.currentSessionId),...(t.currentSessionId?[check('takeOver','确认接管当前执行会话 '+t.currentSessionId)]:[])],'创建新的本地执行会话，保留原会话和 Checkpoint。'));
      actions.append(action('记录进展','task-note',[choose('noteType','记录类型',[['progress','进展'],['decision','决策'],['risk','风险']]),area('text','内容')]));
      if(t.status==='blocked')actions.append(action('解除阻塞','task-unblock',[area('nextStep','恢复后的下一步',t.nextStep)]));
      else actions.append(action('记录阻塞','task-block',[area('reason','阻塞原因'),area('recovery','恢复条件')]));
      actions.append(action('关闭任务','task-close',[choose('outcome','关闭结果',[['completed','完整完成'],['partial','接受部分完成'],['cancelled','取消'],['superseded','已被替代']]),area('reason','关闭原因 / 残余事项','',false),check('confirmed','确认关闭 #'+t.id+'；关闭后不可重新打开')],'关闭由你明确决定；部分完成必须记录残余事项。'));
    }else actions.append(action('修正标题','task-retitle',[text('title','标题（MMDD｜类型｜主题）',t.title)],'只修正标题，不重新打开任务。'));
    heading.append(actions);root.append(heading);
    const next=el('div',undefined,'next-step');next.append(el('h3',t.status==='closed'?'关闭结果':'下一步'),el('p',t.status==='closed'?`${t.closureOutcome} · ${t.closureReason||'约定范围已完成'}`:t.nextStep||'尚未明确下一步'));root.append(next);
    const tabs=el('div',undefined,'tabs');tabs.setAttribute('role','tablist');for(const [key,label] of [['overview','概览'],['sessions','会话'],['worktree','代码现场'],['history','历史']]){const b=button(label,safely(async()=>{activeTab=key;await renderDetail();}));b.setAttribute('role','tab');b.setAttribute('aria-selected',String(activeTab===key));tabs.append(b);}root.append(tabs);
    const content=el('div');root.append(content);const generation=revision,tab=activeTab;
    if(tab==='overview'){
      if(projectsSupported)content.append(section('所属项目',context.project?`${context.project.name} (##${context.project.id})`:'未关联项目'),section('组件范围 ID',(t.componentIds||[]).map(String)));
      content.append(section('目标',t.goal),section('范围',t.scope),section('验收条件',t.acceptanceCriteria));if(t.blockReason)content.append(section('阻塞原因',t.blockReason),section('恢复条件',t.blockRecovery));
      const cp=context.checkpoint;const checkpoint=el('div',undefined,'info-box');checkpoint.append(el('strong','最近 Checkpoint'));
      if(cp){checkpoint.append(el('p',date(cp.createdAt)+' · '+cp.sessionId),section('进展摘要',cp.summary),section('已完成',cp.completed),section('决策',cp.decisions),section('待办',cp.pending),section('风险',cp.risks));}else checkpoint.append(el('p','会话切换前保存 Checkpoint，让下一次继续有据可依。'));
      if(t.currentSessionId&&['in_progress','blocked'].includes(t.status))checkpoint.append(action('保存 Checkpoint','task-checkpoint',[area('summary','当前进展摘要'),area('completed','已完成（每行一项）','',false),area('decisions','已确认决策（每行一项）','',false),area('pending','未完成（每行一项）','',false),area('nextStep','唯一下一步',t.nextStep),area('risks','风险（每行一项）','',false)],'绑定当前 Session，Git HEAD 由服务实时读取。',{sessionId:t.currentSessionId}));content.append(checkpoint);
      const notesArea=el('div',undefined,'section');notesArea.append(el('h3','进展、决策与风险'));content.append(notesArea);
      const notes=await api(`/api/tasks/${t.id}/notes`);if(generation!==revision||tab!==activeTab)return;
      if(!notes.notes.length)notesArea.append(el('p','暂无记录','muted'));
      for(const note of [...notes.notes].reverse()){const row=el('div',undefined,'timeline-item');row.append(el('small',`${{progress:'进展',decision:'决策',risk:'风险'}[note.noteType]} · ${date(note.createdAt)}`),el('p',note.text));notesArea.append(row);}
    } else if(tab==='sessions') {
      content.append(el('p','正在读取会话…','muted'));const data=await api('/api/sessions?taskId='+t.id);if(generation!==revision||tab!==activeTab)return;clear(content);
      if(!data.sessions.length)content.append(section('执行会话','还没有会话。开始任务后可保存 Checkpoint 和绑定客户端。'));
      for(const s of [...data.sessions].reverse()){
        const box=el('div',undefined,'info-box');box.append(el('strong',s.id+(s.id===t.currentSessionId?' · 当前执行':'')),el('p',`${s.endedAt?'已结束':'未结束'} · 开始 ${date(s.startedAt)}${s.endedAt?' · 结束 '+date(s.endedAt):''}`),el('p',s.source?`${s.source} / ${s.externalSessionId||'未绑定外部 ID'}`:'尚未绑定客户端'));if(s.continuedFrom)box.append(el('p','继续自 '+s.continuedFrom));
        const controls=el('div',undefined,'actions');
        if(!s.endedAt&&t.status!=='closed'){if(!s.externalSessionId)controls.append(action('绑定客户端','session-bind',[text('source','来源标识',s.source||'generic'),text('externalSessionId','外部 Session ID')],'一次性显式绑定；不会改变当前执行会话。',{sessionId:s.id}));controls.append(action('结束执行会话','session-close',[check('confirmed','确认结束 '+s.id)],'结束会话不代表任务完成。结束当前会话后须用新会话恢复。',{sessionId:s.id}));}
        const records=el('div');controls.append(button('查看观测',safely(()=>loadEvents(s.id,records))),button('查看导入记录',safely(()=>loadImports(s.id,records))));
        if(t.status!=='closed')controls.append(action('导入已审查记录','session-import-add',[text('path','本机普通文件绝对路径'),check('confirmSensitiveContentReviewed','已检查并移除 Token、Cookie、密码等敏感内容')],'最多 16 MiB，导入副本可显式删除。',{sessionId:s.id}));
        box.append(controls,records);content.append(box);
      }
    } else if(tab==='worktree') {
      content.append(section('数据库登记路径',t.worktreePath||'未关联 Worktree'),section('Repository',t.repositoryPath||'未登记'));
      const w=context.worktreeStatus;
      if(w){content.append(section('实时观察',`观察时间：${date(w.observedAt)}\n存在：${w.exists?'是':'否'}\n分支：${w.branch||'—'}\nHEAD：${w.head||'—'}`));for(const [key,label]of [['staged','已暂存'],['unstaged','未暂存'],['untracked','未跟踪'],['ignored','被忽略文件']])content.append(section(label,w[key]??'观察不可用'));}
      else content.append(el('p',t.worktreePath?'现场观察不可用；请检查上方告警。':'关联后可实时查看分支和文件状态。','muted'));
      if(t.status!=='closed'){
        const controls=el('div',undefined,'actions');
        if(!t.worktreePath){controls.append(action('创建 Worktree','worktree-create',[text('repo','Repository 绝对路径'),text('branch','已有本地分支'),text('path','新 Worktree 绝对路径'),check('confirmed','确认创建上述 Worktree')],'仅使用已有本地分支。不会推送、清理或隐式保存更改。'),action('登记已有 Worktree','worktree-adopt',[text('repo','Repository 绝对路径'),text('path','已有 Worktree 绝对路径'),check('confirmed','确认登记上述路径')],'仅修复数据库引用，不创建或修改文件。'));}
        else controls.append(action('安全删除 Worktree','worktree-remove',[check('confirmed','确认删除 '+t.worktreePath)],'任何 staged、unstaged、untracked 或 ignored 文件都会阻止删除。'),action('解除失效登记','worktree-detach',[text('expectedPath','确认已消失的 Worktree 路径',t.worktreePath),check('confirmed','确认只清除已消失的引用')],'服务会复核 Git 和文件系统；路径仍存在或仍被 Git 登记时拒绝。'));content.append(controls);
      }
    } else {
      content.append(el('p','正在读取历史…','muted'));const data=await api(`/api/tasks/${t.id}/history`);if(generation!==revision||tab!==activeTab)return;clear(content);
      for(const h of [...data.history].reverse()){const item=el('div',undefined,'timeline-item');item.append(el('small',`#${h.sequence} · ${date(h.occurredAt)} · ${h.changeType}`),el('p',h.summary));if(h.payload){const detail=el('details');detail.append(el('summary','查看结构化详情'),el('pre',JSON.stringify(h.payload,null,2)));item.append(detail);}content.append(item);}
    }
  }
  async function loadEvents(id,root,after=0){const data=await api(`/api/sessions/${encodeURIComponent(id)}/events?after=${after}&limit=50`);if(!after)clear(root);if(!data.events.length&&!after)root.append(el('p','暂无观测记录。Hook 仅记录事件元数据，不保存消息正文。','muted'));for(const e of data.events){const row=el('div',undefined,'timeline-item');row.append(el('small',`#${e.sequence} · 发生 ${date(e.occurredAt)} · 接收 ${date(e.receivedAt)}`),el('p',e.kind+' · '+e.eventId));root.append(row);}if(data.hasMore){const more=button('下一页观测',safely(async()=>{more.remove();await loadEvents(id,root,data.nextAfter);}));root.append(more);}if(!after&&data.events.length)root.append(action('清除观测记录','hook-clear',[check('confirmed','确认清除 '+id+' 的观测记录')],'保留最小去重标记，防止重试恢复已删除事件。不保证物理擦除。',{sessionId:id}));}
  async function loadImports(id,root){const data=await api(`/api/sessions/${encodeURIComponent(id)}/imports`);clear(root);if(!data.imports.length)root.append(el('p','暂无导入记录。','muted'));for(const item of data.imports){const node=el('div',undefined,'timeline-item');node.append(el('p',item.sourcePath),el('small',`${item.sizeBytes} 字节 · ${date(item.importedAt)} · SHA-256 ${item.sha256}`),action('删除导入副本','session-import-remove',[check('confirmed','确认删除导入副本 '+item.id)],'只删除数据库副本，不删除源文件。不保证备份或存储介质上的物理擦除。',{importId:item.id}));root.append(node);}}
  function contextText(){const t=context.task,cp=context.checkpoint;return [`# ${t.title||'未命名任务'} (#${t.id})`,`状态：${statuses[t.status]} · version ${t.version}`,'','## 目标',t.goal||'尚未填写','','## 范围',t.scope||'尚未填写','','## 验收',t.acceptanceCriteria||'尚未填写','','## 下一步',t.nextStep||'尚未填写',...(t.blockReason?['','## 阻塞',t.blockReason,'恢复条件：'+t.blockRecovery]:[]),'','## 项目归属',context.project?`${context.project.name} (##${context.project.id})`:'未关联或后端未提供','组件 ID：'+(t.componentIds||[]).join(', '),'','## Checkpoint 后的备注（至多50条）',JSON.stringify(context.notesSinceCheckpoint||[],null,2),...(context.notesTruncated?['备注已截断；请使用完整备注查询。']:[]),'','## Checkpoint',cp?JSON.stringify(cp,null,2):'暂无','','## 执行会话',context.session?JSON.stringify(context.session,null,2):'暂无','','## 实时 Git 状态',context.worktreeStatus?JSON.stringify(context.worktreeStatus,null,2):'未关联或无法观察','','继续前重新读取最新 Task version；本文不授权自动关闭任务。'].join('\n');}
  async function copyContext(){await selectTask(selected);const output=contextText();try{await navigator.clipboard.writeText(output);notify('已复制最新交接上下文。',true);}catch{openAction('copy-context','交接上下文',[area('context','复制下方内容',output)],'选择并复制文本。');$('submit-action').hidden=true;}}
  function sourceContextText(data){return [`# ${data.project.name} (##${data.project.id})`,`源码登记：#${data.source.id}`,`当前目录：${data.resolvedPath||'未提供'}`,`观察时间：${data.observedAt||'未提供'}`,...(data.git?[`Git 分支：${data.git.branch||'未附着分支'}`,`HEAD：${data.git.head||'尚无提交'}`,`工作区：${data.git.dirty?'有改动':'未观察到改动'}`]:['Git 状态：未提供']),'','## 文件导航',...(data.entries||[]).map(e=>e.path),`省略条目：${data.omittedEntries||0}`,'','只读导航，不登记 Worktree，不自动领取或关闭任务。'].join('\n');}
  function showTaskPage(){projectPageVisible=false;$('project-page').hidden=true;$('task-page').hidden=false;}
  async function applyProjectFilter(){
    const generation=++projectFilterRevision,input=$('project-filter').value.trim();
    const result=input?await api('/api/projects/'+encodeURIComponent(input)):null;
    if(generation!==projectFilterRevision||!connected||$('project-filter').value.trim()!==input)return;
    projectFilter=result?'##'+result.project.id:'';$('project-filter').value=projectFilter;
    selected=null;context=null;revision++;clear($('detail'));cursor=null;rows=[];renderList();$('more').hidden=true;await loadList();
  }
  async function loadProjects(append=false,preserveRange=false,background=false){
    const generation=++projectListRevision,target=preserveRange?Math.max(50,projectRows.length):50;
    let after=append?projectCursor:0,data;const fetched=[];
    do{data=await api(`/api/projects?after=${after||0}&limit=50`);if(generation!==projectListRevision||!connected)return false;if(background&&modal){liveDirty=true;return false;}fetched.push(...data.projects);after=data.nextAfter;}while(preserveRange&&data.hasMore&&fetched.length<target);
    projectRows=append?projectRows.concat(fetched):fetched;projectCursor=data.nextAfter;
    renderProjects();$('project-more').hidden=!data.hasMore;return true;
  }
  function renderProjects(){const root=$('project-list'),panel=root.parentElement,top=panel.scrollTop,items=[];if(!projectRows.length)items.push(section('暂无项目','项目用于归类任务；源码登记可以稍后再做。'));for(const p of projectRows){const b=button('',safely(()=>selectProject(p.id)),'task-card'+(selectedProject===p.id?' selected':''));b.setAttribute('aria-current',String(selectedProject===p.id));b.append(el('small',`##${p.id} · revision ${p.revision}`),el('h3',p.name));items.push(b);}root.replaceChildren(...items);panel.scrollTop=top;}
  async function selectProject(id){
    if(selectedProject!==id){projectContext=null;clear($('project-detail'));$('project-detail').append(el('p','正在读取项目…','muted'));}
    selectedProject=id;const generation=++projectDetailRevision;renderProjects();
    const [info,components,sources]=await Promise.all([api(`/api/projects/${id}`),api(`/api/projects/${id}/components`),api(`/api/projects/${id}/sources`)]);
    if(generation!==projectDetailRevision||!connected)return;
    if(info.project.revision!==components.project.revision||info.project.revision!==sources.project.revision)throw new Error('项目在读取期间发生变化，请刷新后重试。');
    projectContext={project:info.project,components:components.components,sources:sources.sources,repositories:sources.repositories};
    const p=info.project,root=$('project-detail');clear(root);const heading=el('div',undefined,'detail-heading');heading.append(el('p',`##${p.id} · revision ${p.revision}`),el('h2',p.name));
    const controls=el('div',undefined,'actions');controls.append(button('查看该项目任务',safely(async()=>{$('project-filter').value='##'+p.id;showTaskPage();await applyProjectFilter();})));
    controls.append(action('修改项目名称','project-rename',[text('name','项目名称',p.name)],'项目 ID 不变，关联任务保留；使用项目 revision 校验。',{projectId:p.id}));
    controls.append(action('添加组件','project-component-add',[text('name','组件名称')],'例如 backend、android；组件只是项目内的范围标记。',{projectId:p.id}));
    const component=()=>({...choose('component','所属组件（可不选）',[['','项目公共源码'],...components.components.map(c=>[c.name,c.name])],''),required:false});
    controls.append(action('登记 Git 源码','project-source-add',[text('worktree','现有 Git 工作区绝对路径'),text('relativePath','源码相对工作区路径','.'),component()],'路径属于服务所在机器；只登记元数据，不上传/复制代码或创建仓库。查询时仍需明确提供工作区路径。',{projectId:p.id,kind:'git'}));
    controls.append(action('登记普通目录','project-source-add',[text('path','非 Git 目录绝对路径'),component()],'路径属于服务所在机器，仅用于非 Git 目录；不会修改目录中的文件。',{projectId:p.id,kind:'directory'}));heading.append(controls);root.append(heading);
    root.append(section('组件',components.components.map(c=>`${c.name} (#${c.id})`)));
    const sourceArea=el('section',undefined,'section');sourceArea.append(el('h3','源码登记'));if(!sources.sources.length)sourceArea.append(el('p','尚未登记源码；不影响项目任务管理。','muted'));
    for(const source of sources.sources){const box=el('div',undefined,'info-box');const repo=(sources.repositories||[]).find(r=>r.id===source.repositoryId);box.append(el('strong','源码 #'+source.id),el('p',source.repositoryId?`Git Repository #${source.repositoryId} · ${repo?.commonDir||'—'}\n相对路径：${source.relativePath}`:source.directoryPath),el('p','组件：'+(components.components.find(c=>c.id===source.componentId)?.name||'项目公共源码')));
      const actions=el('div',undefined,'actions');actions.append(button('查看源码上下文',()=>openAction('project-context','查询项目源码导航',source.repositoryId?[text('worktree','此次查询的 Git 工作区绝对路径')]:[],'只读导航，不读取源码正文。Git 路径须与登记 Repository 的 git worktree list 中本机 checkout 路径一致，不接受网络/设备路径，不从旧 Task 或上次输入推断。',{projectId:p.id,sourceId:source.id})));
      actions.append(action('解除源码登记','project-source-remove',[check('confirmed','确认只解除源码 #'+source.id+' 的登记，不删除文件')],'保留目录与 Git 内容，使用项目 revision 校验。',{projectId:p.id,sourceId:source.id}));box.append(actions);sourceArea.append(box);
    }root.append(sourceArea);
  }
  async function editTaskComponents(task){
    const epoch=modalEpoch,taskRevision=revision;
    const data=await api(`/api/projects/${task.projectId}/components`);
    if(epoch!==modalEpoch||modal||taskRevision!==revision||context?.task.id!==task.id||context.task.version!==task.version||projectPageVisible)return;
    const selectedNames=data.components.filter(c=>(task.componentIds||[]).includes(c.id)).map(c=>c.name).join('\n');
    openAction('task-components','设置组件范围',[area('components','组件名称（每行一项；留空清空）',selectedNames,false),check('confirmed','确认替换组件范围，不改变执行会话或 Worktree')],'可选组件：'+data.components.map(c=>c.name).join('、'));
  }
  $('projects').onclick=safely(async()=>{if(!projectsSupported)return;projectPageVisible=true;$('task-page').hidden=true;$('project-page').hidden=false;await loadProjects(false,true);if(selectedProject)await selectProject(selectedProject);});
  $('back-tasks').onclick=showTaskPage;
  $('create-project').onclick=()=>openAction('project-create','新建项目',[text('name','项目名称')],'只创建项目记录；登记源码、关联任务均为后续显式操作。');
  $('project-more').onclick=safely(()=>loadProjects(true));
  $('project-filter-form').onsubmit=event=>{event.preventDefault();safely(applyProjectFilter)();};
  $('clear-project-filter').onclick=safely(async()=>{$('project-filter').value='';await applyProjectFilter();});
  function openAction(name,title,fields,description='',extra={}){
    if(!canWrite&&!['copy-context','project-context'].includes(name)){notify('当前连接为只读，不能修改任务。');return;}
    modalEpoch++;
    modal={name,fields,extra,task:context?.task?structuredClone(context.task):null,project:extra.projectId&&projectContext?structuredClone(projectContext.project):null};$('action-title').textContent=title;$('action-description').textContent=description;clear($('action-fields'));clear($('action-error'));clear($('conflict'));$('submit-action').hidden=false;$('submit-action').disabled=false;
    for(const f of fields){const label=el('label',f.label);label.htmlFor='field-'+f.key;const input=el(f.type==='textarea'?'textarea':f.type==='select'?'select':'input');input.id='field-'+f.key;input.name=f.key;input.required=!!f.required;if(f.type==='select')for(const [value,label] of f.options){const o=el('option',label);o.value=value;input.append(o);}if(f.type==='checkbox'){input.type='checkbox';label.prepend(input);$('action-fields').append(label);}else{input.value=f.value??'';$('action-fields').append(label,input);}}
    $('action-dialog').showModal();
  }
  function values(){const v={};for(const f of modal.fields){const input=$('field-'+f.key);v[f.key]=f.type==='checkbox'?input.checked:input.value.trim();}return v;}
  function bodyFor(v){const {name,task,extra}=modal;const base=task?{taskId:task.id,expectedVersion:task.version}:{};
    if(name.startsWith('project-')){const projectBase=name==='project-create'?{}:{projectId:modal.project.id,expectedRevision:modal.project.revision};if(name==='project-source-add')return{...projectBase,component:v.component||null,location:extra.kind==='git'?{kind:'git',worktree:v.worktree,relativePath:v.relativePath}:{kind:'directory',path:v.path}};return{...projectBase,...v,...(name==='project-source-remove'?{sourceId:extra.sourceId}:{})};}
    if(name==='task-project')return{...base,project:v.project||null,clear:!v.project,confirmed:v.confirmed};
    if(name==='task-components')return{...base,components:v.components.split('\n').map(x=>x.trim()).filter(Boolean),confirmed:v.confirmed};
    if(name==='task-close'&&!v.reason)v.reason=null;
    if(name==='task-create')return{input:Object.fromEntries(Object.entries(v).filter(([,x])=>x!==''))};
    if(name==='task-update'){const patch=Object.fromEntries(Object.entries(v).filter(([k,x])=>x!==(task[k]??'')));return{...base,patch};}
    if(name==='task-checkpoint'){for(const key of ['completed','decisions','pending','risks'])v[key]=v[key].split('\n').map(x=>x.trim()).filter(Boolean);return{...base,...extra,input:v};}
    if(['session-bind','session-close','session-import-remove','hook-clear'].includes(name))delete base.taskId;
    return{...base,...extra,...v};
  }
  $('action-form').onsubmit=async event=>{
    event.preventDefault();if(!modal)return;const current=modal;const submit=$('submit-action');submit.disabled=true;clear($('action-error'));clear($('conflict'));
    try{
      if(current.name==='project-context'){const v=values(),q=new URLSearchParams({sourceId:current.extra.sourceId});if(v.worktree)q.set('worktree',v.worktree);const result=await api(`/api/projects/${current.extra.projectId}/context?${q}`);if(modal!==current)return;openAction('copy-context','项目源码导航',[area('context','源码目录与文件导航（只读）',sourceContextText(result))],'实时读取登记源码的导航信息；不修改代码，不改变任务执行状态。');$('submit-action').hidden=true;return;}
      const result=await api('/api/commands/'+current.name,bodyFor(values()));if(modal!==current)return;const id=result.task?.id??selected;
      try { if(current.name.startsWith('project-')){await loadProjects(false,true);if(modal!==current)return;if(result.project)await selectProject(result.project.id);}else{await loadList();if(modal!==current)return;if(id)await selectTask(id);}notify('已保存。',true); } catch(readError) {notify('提交已成功，但刷新失败。请手动刷新，不要重复提交。\n'+readError.message);}
      if(modal===current){$('action-dialog').close();modal=null;}
    }catch(error){
      if(modal!==current)return;$('action-error').textContent=error.message;
      if(error.code==='VERSION_CONFLICT'){
        try{const isProject=!!current.project;const latest=await api(isProject?'/api/projects/'+current.project.id:'/api/tasks/'+current.task.id);if(modal!==current)return;const box=el('div',undefined,'notice');box.append(el('p','你的填写内容已保留。请对照服务中的最新内容，调整后再提交。'),el('pre',JSON.stringify(isProject?latest.project:latest.task,null,2)));box.append(button('采用此版本，重新审查后提交',()=>{if(isProject)current.project=latest.project;else current.task=latest.task;box.remove();$('action-error').textContent='已采用版本 '+(isProject?latest.project.revision:latest.task.version)+'。请审查填写内容，然后再次点击确认提交。';submit.disabled=false;}));$('conflict').append(box);}catch(e){$('action-error').append(el('p',e.message));}
      } else if(error.uncertain){
        const box=el('div',undefined,'notice');if(error.details)box.append(el('pre',JSON.stringify(error.details,null,2)));box.append(button('刷新列表和现场',safely(async()=>{if(current.project||current.name==='project-create'){await loadProjects(false,true);if(current.project)await selectProject(current.project.id);}else{await loadList();if(selected)await selectTask(selected);}})),button('已核对执行结果，允许再次提交',()=>{submit.disabled=false;box.remove();}));$('conflict').append(box);
      }else submit.disabled=false;
    }
  };
  async function connect(value){
    $('login-error').textContent='';$('credential').value='';stopLive();
    try{
      if(value)await api('/api/login',{}, {'X-Steward-Token':value});
      const access=await api('/api/access');localAccess=access.local===true;canWrite=access.role==='admin';projectsSupported=access.projectManagement===true;$('projects').hidden=!projectsSupported;$('project-filter-form').hidden=!projectsSupported;$('create-project').hidden=!canWrite;await loadList();
    }catch(e){$('login-error').textContent=!value&&e.code==='UNAUTHORIZED'?'':e.message;return;}
    connected=true;$('create').hidden=!canWrite;$('revoke-browsers').hidden=!canWrite;$('logout').hidden=localAccess;$('access-role').textContent=localAccess?'本机 · 管理员':canWrite?'管理员':'只读';
    $('login').hidden=true;$('workspace').hidden=false;
    if(rows.length)try{await selectTask(rows[0].id);}catch(e){notify(e.message);}
    if(connected)startLive();
  }
  async function logout(){await api('/api/logout',{});resetConnection();}
  $('connect-form').onsubmit=async event=>{event.preventDefault();const b=event.submitter;if(b)b.disabled=true;try{await connect($('credential').value.trim());}finally{if(b)b.disabled=false;}};
  $('logout').onclick=safely(logout);
  $('revoke-browsers').onclick=safely(async()=>{if(canWrite&&confirm('撤销所有已保存的浏览器授权？不影响本机免登录和凭据文件。')){await api('/api/browser-sessions/revoke',{});if(localAccess)notify('已撤销保存的浏览器授权。');else resetConnection();}});$('refresh').onclick=safely(async()=>{notify('');if(projectPageVisible){await loadProjects(false,true);if(selectedProject)await selectProject(selectedProject);}else{await loadList();if(selected)await selectTask(selected);}});
  $('create').onclick=()=>openAction('task-create','新建任务',[...taskFields(null),...(projectsSupported?[text('project','项目（##编号或唯一名称，可留空）',projectFilter,false)]:[])],'可以先创建最小任务，随后补充目标、范围和验收条件。');
  $('search-form').onsubmit=event=>{event.preventDefault();query=$('search').value.trim();safely(()=>loadList())();};$('more').onclick=safely(()=>loadList(true));
  for(const b of document.querySelectorAll('[data-view]'))b.onclick=safely(async()=>{view=b.dataset.view;document.querySelectorAll('[data-view]').forEach(x=>x.setAttribute('aria-pressed',String(x===b)));await loadList();});
  for(const id of ['cancel','cancel-bottom'])$(id).onclick=()=>$('action-dialog').close();$('action-dialog').addEventListener('close',()=>{if($('action-dialog').open)return;modal=null;if(liveDirty)scheduleLiveRefresh();});
  function stopLive(){
    if(liveAbort)liveAbort.abort();liveAbort=null;
    if(liveTimer)clearTimeout(liveTimer);liveTimer=null;liveDirty=false;
    $('live-state').textContent='未连接';
  }
  function scheduleLiveRefresh(){
    liveDirty=true;if(!connected||liveTimer||liveRefreshing)return;
    if(modal){$('live-state').textContent='有更新，关闭表单后刷新';return;}
    liveTimer=setTimeout(async()=>{
      liveTimer=null;if(!connected||modal)return;
      liveDirty=false;liveRefreshing=true;const connection=liveAbort;
      try{if(projectPageVisible){if(await loadProjects(false,true,true)&&connection===liveAbort&&selectedProject&&!modal)await selectProject(selectedProject);return;}if(!await loadList(false,true)||connection!==liveAbort)return;if(modal){liveDirty=true;return;}if(selected)await selectTask(selected);}
      catch(e){if(connection===liveAbort)notify(e.message);}
      finally{liveRefreshing=false;if(liveDirty&&connected)scheduleLiveRefresh();}
    },250);
  }
  function startLive(){
    stopLive();const controller=new AbortController();liveAbort=controller;
    void (async()=>{
      while(!controller.signal.aborted){
        let reader;
        try{
          const response=await fetch('/api/events',{headers:{'X-Steward-UI-Contract':'1'},credentials:'same-origin',signal:controller.signal,cache:'no-store'});
          if(controller.signal.aborted)return;
          if(response.status===401){resetConnection();$('login-error').textContent='浏览器授权已失效，请重新连接。';return;}
          if(!response.ok||!response.body)throw new Error('event stream unavailable');
          $('live-state').textContent='实时同步';reader=response.body.getReader();const decoder=new TextDecoder();let buffer='';
          while(!controller.signal.aborted){
            const {done,value}=await reader.read();if(done)break;
            buffer+=decoder.decode(value,{stream:true}).replace(/\r/g,'');
            let end;while((end=buffer.indexOf('\n\n'))!==-1){
              const frame=buffer.slice(0,end);buffer=buffer.slice(end+2);
              if(frame.split('\n').some(line=>line==='event: changed'))scheduleLiveRefresh();
              if(frame.split('\n').some(line=>line==='event: unauthorized')){void connect();return;}
              if(frame.split('\n').some(line=>line==='event: unavailable'))throw new Error('event stream unavailable');
            }
            if(buffer.length>8192)throw new Error('invalid event stream');
          }
        }catch{/* Only the read-only notification channel reconnects; writes are never replayed. */}
        finally{if(reader)await reader.cancel().catch(()=>{});}
        if(controller.signal.aborted)return;
        $('live-state').textContent='同步断开，正在重连';
        await new Promise(resolve=>{const finish=()=>{clearTimeout(timer);controller.signal.removeEventListener('abort',finish);resolve();};const timer=setTimeout(finish,2000);controller.signal.addEventListener('abort',finish,{once:true});});
      }
    })();
  }
  async function autoConnect(code){
    resetConnection();
    const controls=Array.from($('connect-form').querySelectorAll('input,button'));
    controls.forEach(control=>control.disabled=true);
    try{
      if(!/^[a-f0-9]{64}$/.test(code))throw new Error('invalid connection code');
      await api('/api/connect',{}, {'X-Steward-Connect':code});
      await connect();
    }catch{$('login-error').textContent='自动连接未完成或连接链接已失效。请使用终端提示的凭据文件手动连接，或重新启动工作台。';}
    finally{controls.forEach(control=>control.disabled=false);}
  }
  // UI activation is separate from data invalidations. Never replay a write.
  function startUiUpdates(){
    const loaded=document.querySelector?.('meta[name="steward-ui-release"]')?.content;
    if(!loaded)return;
    let held=false, banner=null, checking=false;
    const editing=()=>!!modal||!!$('credential').value||$('search').value.trim()!==query||$('project-filter').value.trim()!==projectFilter;
    async function checkUpdate(){
      if(checking)return;checking=true;
      try{
        const response=await fetch('/ui/status',{cache:'no-store',credentials:'same-origin',signal:AbortSignal.timeout(5000)});
        if(!response.ok)return;
        const status=await response.json();
        if(status.packageFormat!==1||!Number.isInteger(status.apiContract)||typeof status.release!=='string')return;
        if(status.release===loaded){banner?.remove();banner=null;held=false;return;}
        if(!held&&!editing()&&!pendingWrites&&!uncertainWrite){location.reload();return;}
        held=true;
        if(!banner){
          banner=el('aside',undefined,'notice');banner.setAttribute('role','status');
          banner.append(el('span','新版 UI 已就绪。当前输入已保留；请完成编辑并核对提交结果后刷新。 '),button('刷新采用新版',()=>{
            if(pendingWrites){alert('提交仍在进行，请等待结果。');return;}
            if(confirm('重新加载将丢弃未提交输入。已完成编辑并核对提交结果，继续？'))location.reload();
          }));document.body.prepend(banner);
        }
      }catch{/* An unavailable update check must not interrupt business work. */}
      finally{checking=false;setTimeout(checkUpdate,10000);}
    }
    setTimeout(checkUpdate,10000);
  }
  startUiUpdates();
  const code=new URLSearchParams(location.hash.slice(1)).get('connect');
  if(code!==null){
    // Remove the one-use secret before any exchange; never send it in a query string.
    history.replaceState(null,'',location.pathname+location.search);
    void autoConnect(code);
  }else{void connect();}
})();
