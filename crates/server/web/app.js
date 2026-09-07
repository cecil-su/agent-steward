'use strict';
(() => {
  const $ = id => document.getElementById(id);
  const statuses = {open:'待开始',in_progress:'进行中',blocked:'有阻塞',closed:'已关闭'};
  let warningText='', listRevision=0;
  let pendingWrites=0, uncertainWrite=false;
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
  function resetConnection(){stopLive();canWrite=false;connected=false;warningText='';listRevision++;rows=[];selected=null;context=null;revision++;$('workspace').hidden=true;$('login').hidden=false;$('credential').value='';clear($('task-list'));clear($('detail'));$('action-dialog').close();modal=null;}
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
  async function loadList(append=false){
    const generation=++listRevision;const search=new URLSearchParams(view==='closed'?{status:'closed',pageSize:'30'}:{view,pageSize:'30'});if(query)search.set('query',query);if(append&&cursor)search.set('cursor',cursor);
    const data=await api('/api/tasks?'+search);if(generation!==listRevision)return;rows=append?rows.concat(data.tasks):data.tasks;cursor=data.nextCursor;
    renderList();$('more').hidden=!data.hasMore;
  }
  function renderList(){clear($('task-list'));$('task-count').textContent=rows.length+' 项';
    if(!rows.length){const empty=el('div',undefined,'empty');empty.append(el('h2','这里还没有任务'),el('p','新建任务，或切换视图和搜索条件。'));$('task-list').append(empty);}
    for(const task of rows){const b=button('',safely(()=>selectTask(task.id)),'task-card'+(selected===task.id?' selected':''));b.setAttribute('aria-current',String(selected===task.id));const meta=el('div',undefined,'card-meta');meta.append(el('span','#'+task.id),badge(task.status));b.append(meta,el('h3',task.title||'未命名任务'),el('p',task.nextStep||task.goal||'等待补充目标和下一步'));$('task-list').append(b);}
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
  function contextText(){const t=context.task,cp=context.checkpoint;return [`# ${t.title||'未命名任务'} (#${t.id})`,`状态：${statuses[t.status]} · version ${t.version}`,'','## 目标',t.goal||'尚未填写','','## 范围',t.scope||'尚未填写','','## 验收',t.acceptanceCriteria||'尚未填写','','## 下一步',t.nextStep||'尚未填写',...(t.blockReason?['','## 阻塞',t.blockReason,'恢复条件：'+t.blockRecovery]:[]),'','## Checkpoint',cp?JSON.stringify(cp,null,2):'暂无','','## 执行会话',context.session?JSON.stringify(context.session,null,2):'暂无','','## 实时 Git 状态',context.worktreeStatus?JSON.stringify(context.worktreeStatus,null,2):'未关联或无法观察','','继续前重新读取最新 Task version；本文不授权自动关闭任务。'].join('\n');}
  async function copyContext(){await selectTask(selected);const output=contextText();try{await navigator.clipboard.writeText(output);notify('已复制最新交接上下文。',true);}catch{openAction('copy-context','交接上下文',[area('context','复制下方内容',output)],'选择并复制文本。');$('submit-action').hidden=true;}}
  function openAction(name,title,fields,description='',extra={}){
    if(!canWrite&&name!=='copy-context'){notify('当前连接为只读，不能修改任务。');return;}
    modal={name,fields,extra,task:context?.task?structuredClone(context.task):null};$('action-title').textContent=title;$('action-description').textContent=description;clear($('action-fields'));clear($('action-error'));clear($('conflict'));$('submit-action').hidden=false;$('submit-action').disabled=false;
    for(const f of fields){const label=el('label',f.label);label.htmlFor='field-'+f.key;const input=el(f.type==='textarea'?'textarea':f.type==='select'?'select':'input');input.id='field-'+f.key;input.name=f.key;input.required=!!f.required;if(f.type==='select')for(const [value,label] of f.options){const o=el('option',label);o.value=value;input.append(o);}if(f.type==='checkbox'){input.type='checkbox';label.prepend(input);$('action-fields').append(label);}else{input.value=f.value??'';$('action-fields').append(label,input);}}
    $('action-dialog').showModal();
  }
  function values(){const v={};for(const f of modal.fields){const input=$('field-'+f.key);v[f.key]=f.type==='checkbox'?input.checked:input.value.trim();}return v;}
  function bodyFor(v){const {name,task,extra}=modal;const base=task?{taskId:task.id,expectedVersion:task.version}:{};
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
      const result=await api('/api/commands/'+current.name,bodyFor(values()));const id=result.task?.id??selected;
      try { await loadList();if(id)await selectTask(id);notify('已保存。',true); } catch(readError) {notify('提交已成功，但刷新失败。请手动刷新，不要重复提交。\n'+readError.message);}
      $('action-dialog').close();modal=null;
    }catch(error){
      if(modal!==current)return;$('action-error').textContent=error.message;
      if(error.code==='VERSION_CONFLICT'){
        try{const latest=await api('/api/tasks/'+current.task.id);const box=el('div',undefined,'notice');box.append(el('p','你的填写内容已保留。请对照服务中的最新内容，调整后再提交。'),el('pre',JSON.stringify(latest.task,null,2)));box.append(button('采用此版本，重新审查后提交',()=>{current.task=latest.task;box.remove();$('action-error').textContent='已采用版本 '+latest.task.version+'。请审查填写内容，然后再次点击确认提交。';submit.disabled=false;}));$('conflict').append(box);}catch(e){$('action-error').append(el('p',e.message));}
      } else if(error.uncertain){
        const box=el('div',undefined,'notice');if(error.details)box.append(el('pre',JSON.stringify(error.details,null,2)));box.append(button('刷新列表和现场',safely(async()=>{await loadList();if(selected)await selectTask(selected);})),button('已核对执行结果，允许再次提交',()=>{submit.disabled=false;box.remove();}));$('conflict').append(box);
      }else submit.disabled=false;
    }
  };
  async function connect(value){
    $('login-error').textContent='';$('credential').value='';stopLive();
    try{
      if(value)await api('/api/login',{}, {'X-Steward-Token':value});
      const access=await api('/api/access');localAccess=access.local===true;canWrite=access.role==='admin';await loadList();
    }catch(e){$('login-error').textContent=!value&&e.code==='UNAUTHORIZED'?'':e.message;return;}
    connected=true;$('create').hidden=!canWrite;$('revoke-browsers').hidden=!canWrite;$('logout').hidden=localAccess;$('access-role').textContent=localAccess?'本机 · 管理员':canWrite?'管理员':'只读';
    $('login').hidden=true;$('workspace').hidden=false;
    if(rows.length)try{await selectTask(rows[0].id);}catch(e){notify(e.message);}
    if(connected)startLive();
  }
  async function logout(){await api('/api/logout',{});resetConnection();}
  $('connect-form').onsubmit=async event=>{event.preventDefault();const b=event.submitter;if(b)b.disabled=true;try{await connect($('credential').value.trim());}finally{if(b)b.disabled=false;}};
  $('logout').onclick=safely(logout);
  $('revoke-browsers').onclick=safely(async()=>{if(canWrite&&confirm('撤销所有已保存的浏览器授权？不影响本机免登录和凭据文件。')){await api('/api/browser-sessions/revoke',{});if(localAccess)notify('已撤销保存的浏览器授权。');else resetConnection();}});$('refresh').onclick=safely(async()=>{notify('');await loadList();if(selected)await selectTask(selected);});
  $('create').onclick=()=>openAction('task-create','新建任务',taskFields(null),'可以先创建最小任务，随后补充目标、范围和验收条件。');
  $('search-form').onsubmit=event=>{event.preventDefault();query=$('search').value.trim();safely(()=>loadList())();};$('more').onclick=safely(()=>loadList(true));
  for(const b of document.querySelectorAll('[data-view]'))b.onclick=safely(async()=>{view=b.dataset.view;document.querySelectorAll('[data-view]').forEach(x=>x.setAttribute('aria-pressed',String(x===b)));await loadList();});
  for(const id of ['cancel','cancel-bottom'])$(id).onclick=()=>$('action-dialog').close();$('action-dialog').addEventListener('close',()=>{modal=null;if(liveDirty)scheduleLiveRefresh();});
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
      try{await loadList();if(connection!==liveAbort)return;if(modal){liveDirty=true;return;}if(selected)await selectTask(selected);}
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
    const editing=()=>!!modal||!!$('credential').value||$('search').value.trim()!==query;
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
