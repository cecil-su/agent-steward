const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const {webcrypto}=require('node:crypto');
const source=fs.readFileSync(path.join(__dirname,'../../web/app.js'),'utf8');
const settle=()=>new Promise(resolve=>setImmediate(resolve));
async function until(check){for(let i=0;i<200;i++){if(check())return;await new Promise(r=>setTimeout(r,10));}assert(check(),'timed out waiting for project state');}
function fixture({reader=false,supported=true,tasksReply,queuedClose=false}={}){
  const closeEvents=[];
  const nodes=new Map(),requests=[],projects=[],sources=[],components=[];
  const task={id:1,title:'0907｜功能｜示例任务',status:'open',version:1,projectId:null,componentIds:[],currentSessionId:null};
  let loseNext=false,intercept,eventController;
  const panel={scrollTop:0};
  function node(tag='div'){
    const handlers={};let id='';
    return {tag,children:[],attributes:{},textContent:'',value:'',hidden:false,disabled:false,checked:false,parentElement:panel,scrollTop:0,
      get id(){return id;},set id(v){id=v;nodes.set(v,this);},
      append(...items){this.children.push(...items);},prepend(...items){this.children.unshift(...items);},replaceChildren(...items){this.children=items;},
      setAttribute(k,v){this.attributes[k]=v;},querySelectorAll(){return [];},addEventListener(k,f){handlers[k]=f;},
      showModal(){this.open=true;},close(){if(!this.open)return;this.open=false;if(queuedClose)closeEvents.push(()=>handlers.close?.());else handlers.close?.();},remove(){this.removed=true;}};
  }
  const get=id=>{if(!nodes.has(id))nodes.set(id,node());return nodes.get(id);};
  const ok=data=>({ok:true,status:200,json:async()=>({ok:true,data:structuredClone(data),warnings:[]})});
  function project(ref){return projects.find(p=>'##'+p.id===ref||String(p.id)===ref||p.name===ref);}
  vm.runInNewContext(source,{
    document:{getElementById:get,createElement:node,querySelectorAll:()=>[],querySelector:()=>null,scrollingElement:{scrollTop:0}},
    location:{hash:''},sessionStorage:{removeItem(){}},crypto:webcrypto,Uint8Array,URLSearchParams,URL,AbortSignal,AbortController,TextDecoder,setTimeout,clearTimeout,structuredClone,confirm:()=>true,
    fetch:async(url,options)=>{
      const body=options.body?JSON.parse(options.body):null;requests.push({url,body});if(intercept)await intercept(url,body);
      if(url==='/api/events')return {ok:true,body:new ReadableStream({start(c){eventController=c;options.signal.addEventListener('abort',()=>c.close(),{once:true});}})};
      if(url==='/api/access')return ok({role:reader?'reader':'admin',local:false,projectManagement:supported});
      if(url==='/api/logout')return ok({});
      const u=new URL(url,'http://fixture');
      if(u.pathname==='/api/tasks')return ok(tasksReply?tasksReply(u,task):{tasks:!u.searchParams.get('project')||u.searchParams.get('project')==='##'+task.projectId?[task]:[],hasMore:false,nextCursor:null});
      if(url==='/api/tasks/1/context')return ok({task,project:projects.find(p=>p.id===task.projectId)||null,notesSinceCheckpoint:[{noteType:'progress',text:'new note'}],notesTruncated:false,session:null,worktreeStatus:null,checkpoint:null});
      if(url==='/api/tasks/1/notes')return ok({notes:[]});
      if(url==='/api/tasks/1')return ok({task});
      if(u.pathname==='/api/projects'){
        const rows=projects.filter(p=>p.id>Number(u.searchParams.get('after')||0)),limit=Number(u.searchParams.get('limit')||50);
        return ok({projects:rows.slice(0,limit),hasMore:rows.length>limit,nextAfter:rows.length>limit?rows[limit-1].id:null});
      }
      if(u.pathname.startsWith('/api/projects/')){
        const parts=u.pathname.split('/').map(decodeURIComponent),p=project(parts[3]);assert(p,'project request must resolve');
        if(!parts[4])return ok({project:p});
        if(parts[4]==='components')return ok({project:p,components});
        if(parts[4]==='sources')return ok({project:p,sources,repositories:[]});
        if(parts[4]==='context')return ok({project:p,source:sources[0],entries:[],reuseAllowed:false});
      }
      if(url.startsWith('/api/commands/')){
        assert(!reader,'reader must not send mutations');
        const name=url.split('/').at(-1);let result;
        if(name.startsWith('project-'))assert(!Object.hasOwn(body,'taskId')&&!Object.hasOwn(body,'expectedVersion'));
        if(name==='project-create'){projects.push({id:projects.length+1,name:body.name,revision:1});result={project:projects.at(-1)};}
        else if(name.startsWith('project-')){
          const p=projects.find(p=>p.id===body.projectId);assert(p);
          if(p.revision!==body.expectedRevision)return {ok:false,status:409,json:async()=>({ok:false,error:{code:'VERSION_CONFLICT',message:'stale project',details:{entityType:'Project',projectId:p.id,currentRevision:p.revision}}})};
          if(name==='project-rename')p.name=body.name;
          else if(name==='project-source-add'){assert.equal(body.location.kind,'directory');sources.push({id:1,directoryPath:body.location.path,componentId:null,repositoryId:null});}
          else if(name==='project-source-remove'){assert.equal(body.confirmed,true);sources.splice(0);}
          else assert.fail(name);
          p.revision++;result={project:p};
        }else if(name==='task-project'){
          assert.equal(body.expectedVersion,task.version);assert.equal(body.confirmed,true);task.projectId=body.clear?null:project(body.project).id;task.componentIds=[];task.version++;result={task};
        }else assert.fail(name);
        if(loseNext){loseNext=false;throw Error('synthetic response lost');}
        return ok(result);
      }
      assert.fail('unexpected request '+url);
    },
  });
  const text=n=>n.textContent+(n.children||[]).map(text).join('');
  function find(root,label){if(root.tag==='button'&&root.textContent===label)return root;for(const child of root.children||[]){const hit=find(child,label);if(hit)return hit;}}
  return {flushCloseEvents:()=>{while(closeEvents.length)closeEvents.shift()();},get,requests,projects,task,sources,text,button:(id,label)=>{const b=find(get(id),label);assert(b,'button not found: '+label);return b;},submit:()=>get('action-form').onsubmit({preventDefault(){}}),lose:()=>loseNext=true,intercept:fn=>intercept=fn,change:()=>eventController.enqueue(new TextEncoder().encode('event: changed\ndata: refresh\n\n'))};
}
async function create(f,name='Mailroom'){
  await f.get('projects').onclick();f.get('create-project').onclick();f.get('field-name').value=name;await f.submit();
}

test('project page completes create/source/associate/filter/context journey without automatic task execution',async()=>{
  const f=fixture();await settle();
  try{
    assert.equal(f.get('projects').hidden,false);
    await create(f,'<script>literal project</script>');
    assert.equal(f.projects.length,1);assert.equal(f.task.version,1);
    assert(f.text(f.get('project-detail')).includes('<script>literal project</script>'));
    f.button('project-detail','登记普通目录').onclick();assert.equal(f.get('field-component').required,false);f.get('field-path').value='C:/synthetic/docs';await f.submit();
    const added=f.requests.find(r=>r.url.endsWith('project-source-add')).body;
    assert.deepEqual(added,{projectId:1,expectedRevision:1,component:null,location:{kind:'directory',path:'C:/synthetic/docs'}});
    f.button('project-detail','查看源码上下文').onclick();const writes=f.requests.filter(r=>r.body).length;await f.submit();
    assert(f.get('field-context').value.includes('只读导航'));assert.equal(f.requests.filter(r=>r.body).length,writes);f.get('cancel').onclick();
    f.get('back-tasks').onclick();f.button('detail','关联项目').onclick();f.get('field-project').value='##1';f.get('field-confirmed').checked=true;await f.submit();
    assert.equal(f.task.projectId,1);assert.equal(f.task.currentSessionId,null);
    await f.get('projects').onclick();await f.button('project-detail','查看该项目任务').onclick();
    assert.equal(f.get('project-filter').value,'##1');assert(f.requests.some(r=>r.url.includes('project=%23%231')));
    await f.get('task-list').children[0].onclick();await f.button('detail','复制交接上下文').onclick();
    assert(f.get('field-context').value.includes('##1'));assert(f.get('field-context').value.includes('new note'));f.get('cancel').onclick();
  }finally{await f.get('logout').onclick();}
});

test('project conflict retains inputs and adopts only the project revision; unknown writes are not replayed',async()=>{
  const f=fixture();await settle();
  try{
    await create(f);f.button('project-detail','修改项目名称').onclick();f.get('field-name').value='My draft';f.projects[0].revision++;f.projects[0].name='External';
    await f.submit();assert.equal(f.get('field-name').value,'My draft');assert.equal(f.get('submit-action').disabled,true);
    assert(f.requests.some(r=>r.url==='/api/projects/1'&&!r.body));
    f.button('conflict','采用此版本，重新审查后提交').onclick();await f.submit();
    assert.equal(f.projects[0].name,'My draft');assert.equal(f.projects[0].revision,3);assert.equal(f.task.version,1);
    f.button('project-detail','修改项目名称').onclick();f.get('field-name').value='Committed but unknown';f.lose();
    const count=f.requests.filter(r=>r.body).length;await f.submit();await settle();
    assert.equal(f.requests.filter(r=>r.body).length,count+1);assert.equal(f.get('submit-action').disabled,true);
    assert.equal(f.get('field-name').value,'Committed but unknown');f.get('cancel').onclick();
  }finally{await f.get('logout').onclick();}
});

test('late post-save refresh cannot close a newer project form',async()=>{
  const f=fixture();await settle();let release;
  try{
    await create(f);f.button('project-detail','修改项目名称').onclick();f.get('field-name').value='Saved';
    f.intercept((url,body)=>{if(!body&&url.startsWith('/api/projects?'))return new Promise(resolve=>release=resolve);});
    const saving=f.submit();await settle();assert(release);
    f.get('cancel').onclick();f.get('create-project').onclick();f.get('field-name').value='Keep this new draft';
    f.intercept(null);release();await saving;
    assert.equal(f.get('action-dialog').open,true);assert.equal(f.get('field-name').value,'Keep this new draft');assert.equal(f.get('action-title').textContent,'新建项目');f.get('cancel').onclick();
  }finally{release?.();await f.get('logout').onclick();}
});

test('queued old close preserves a reopened draft and its ability to submit',async()=>{
  const f=fixture({queuedClose:true});await settle();
  try{
    await f.get('projects').onclick();f.get('create-project').onclick();f.get('cancel').onclick();
    f.get('create-project').onclick();f.get('field-name').value='Keep reopened draft';
    f.flushCloseEvents();assert.equal(f.get('action-dialog').open,true);
    assert.equal(f.get('field-name').value,'Keep reopened draft');await f.submit();
    assert.equal(f.projects.length,1);assert.equal(f.projects[0].name,'Keep reopened draft');
    assert.equal(f.requests.filter(r=>r.url==='/api/commands/project-create').length,1);
    assert.equal(f.get('action-dialog').open,false);f.flushCloseEvents();
    // A close of the current dialog must still clear its action state.
    f.get('create-project').onclick();f.get('field-name').value='Discard';f.get('cancel').onclick();
    f.flushCloseEvents();await f.submit();assert.equal(f.projects.length,1);
  }finally{await f.get('logout').onclick();f.flushCloseEvents();}
});

test('queued old close cannot discard the completion of a newer pending write',async()=>{
  const f=fixture({queuedClose:true});await settle();let release;
  try{
    await f.get('projects').onclick();f.get('create-project').onclick();f.get('cancel').onclick();
    f.get('create-project').onclick();f.get('field-name').value='New write';
    f.intercept(url=>url==='/api/commands/project-create'?new Promise(resolve=>release=resolve):undefined);
    const saving=f.submit();await settle();assert(release);f.flushCloseEvents();
    assert.equal(f.get('action-dialog').open,true);assert.equal(f.get('submit-action').disabled,true);
    f.intercept(null);release();await saving;
    assert.equal(f.projects.length,1);assert.equal(f.get('action-dialog').open,false);
    assert(f.text(f.get('project-detail')).includes('New write'));f.flushCloseEvents();
  }finally{release?.();await f.get('logout').onclick();f.flushCloseEvents();}
});

test('project pagination and SSE retain loaded range, selection and an open draft',async()=>{
  const f=fixture();for(let i=1;i<=60;i++)f.projects.push({id:i,name:'Project '+i,revision:1});await settle();
  try{
    await f.get('projects').onclick();assert.equal(f.get('project-list').children.length,50);
    await f.get('project-more').onclick();assert.equal(f.get('project-list').children.length,60);
    await f.get('project-list').children[57].onclick();f.button('project-detail','修改项目名称').onclick();f.get('field-name').value='unsaved';
    f.projects.push({id:61,name:'External new project',revision:1});f.change();await settle();
    assert.equal(f.get('field-name').value,'unsaved');assert.equal(f.get('project-list').children.length,60);
    f.get('cancel').onclick();await until(()=>f.get('project-list').children.length===61);
    assert.equal(f.get('project-list').children[57].attributes['aria-current'],'true');assert.equal(f.task.version,1);
  }finally{await f.get('logout').onclick();}
});

test('a late project reference lookup preserves newer unapplied filter input',async()=>{
  const f=fixture();f.projects.push({id:1,name:'Project A',revision:1});await settle();let release;
  try{
    f.intercept(url=>{if(url==='/api/projects/%23%231')return new Promise(resolve=>release=resolve);});
    f.get('project-filter').value='##1';f.get('project-filter-form').onsubmit({preventDefault(){}});await settle();assert(release);
    f.get('project-filter').value='new draft';f.intercept(null);release();await settle();
    assert.equal(f.get('project-filter').value,'new draft');assert(!f.requests.some(r=>r.url.includes('project=%23%231')));
  }finally{release?.();await f.get('logout').onclick();}
});

test('late component lookup never replaces a newer form or its draft',async()=>{
  const f=fixture();f.projects.push({id:1,name:'P1',revision:1});f.task.projectId=1;await settle();let release;
  try{
    f.intercept(url=>url==='/api/projects/1/components'?new Promise(resolve=>release=resolve):undefined);
    const opening=f.button('detail','设置组件范围').onclick();await settle();assert(release);
    f.button('detail','编辑任务').onclick();f.get('field-goal').value='Keep this draft';
    f.intercept(null);release();await opening;
    assert.equal(f.get('action-title').textContent,'编辑任务');assert.equal(f.get('field-goal').value,'Keep this draft');
    assert(f.get('action-fields').children.some(n=>n.id==='field-goal'));f.get('cancel').onclick();
  }finally{release?.();await f.get('logout').onclick();}
});

test('failed project switch clears previous rows and pagination before a recovered load-more',async()=>{
  const f=fixture({tasksReply:(u,t)=>u.searchParams.has('project')?{tasks:[{...t,id:101,projectId:1}],hasMore:false,nextCursor:null}:{tasks:Array.from({length:30},(_,i)=>({...t,id:i+1})),hasMore:true,nextCursor:'older'}});
  f.projects.push({id:1,name:'P1',revision:1},{id:2,name:'P2',revision:1});f.task.projectId=2;await settle();
  const cards=()=>f.get('task-list').children.filter(n=>n.tag==='button');
  try{
    assert.equal(cards().length,30);assert.equal(f.get('more').hidden,false);
    f.intercept(url=>{if(url.startsWith('/api/tasks?')&&url.includes('project=%23%231'))throw Error('synthetic offline');});
    f.get('project-filter').value='##1';f.get('project-filter-form').onsubmit({preventDefault(){}});await settle();
    assert.equal(cards().length,0);assert.equal(f.get('more').hidden,true);
    f.intercept(null);await f.get('more').onclick();
    assert.equal(cards().length,1);assert.equal(cards()[0].children[0].children[0].textContent,'#101');
    assert.equal(f.get('project-filter').value,'##1');
  }finally{await f.get('logout').onclick();}
});

test('reader can inspect projects and navigation; old backends do not expose unsupported project controls',async()=>{
  const f=fixture({reader:true});f.projects.push({id:1,name:'Read only',revision:1});f.sources.push({id:1,directoryPath:'C:/fixture',repositoryId:null});await settle();
  try{
    await f.get('projects').onclick();await f.get('project-list').children[0].onclick();
    assert.equal(f.get('create-project').hidden,true);assert.equal(f.button('project-detail','修改项目名称').hidden,true);
    f.get('create-project').onclick();assert.equal(f.get('action-dialog').open,undefined);
    f.button('project-detail','查看源码上下文').onclick();await f.submit();assert(f.get('field-context').value.includes('Read only'));
    assert.equal(f.requests.filter(r=>r.body).length,0);f.get('cancel').onclick();
  }finally{await f.get('logout').onclick();}
  const old=fixture({supported:false});await settle();
  try{assert.equal(old.get('projects').hidden,true);assert.equal(old.get('project-filter-form').hidden,true);await old.get('projects').onclick();assert(!old.requests.some(r=>r.url.startsWith('/api/projects')));}finally{await old.get('logout').onclick();}
});


test('returning from projects refreshes hidden task changes and preserves loaded pages and selection',async()=>{
  const f=fixture({tasksReply:(u,t)=>{
    const offset=u.searchParams.has('cursor')?30:0;
    return {tasks:Array.from({length:30},(_,i)=>({...t,id:offset+i+1})),hasMore:offset===0,nextCursor:offset===0?'page-two':null};
  }});
  await settle();
  try{
    await f.get('more').onclick();
    assert.equal(f.get('task-list').children.length,60);
    await f.get('projects').onclick();
    const before=f.requests.filter(r=>r.url.startsWith('/api/projects?')).length;
    f.task.title='Changed externally';f.task.version++;f.task.nextStep='New next step';
    f.change();
    await until(()=>f.requests.filter(r=>r.url.startsWith('/api/projects?')).length>before);
    await settle();
    await f.get('back-tasks').onclick();
    assert.equal(f.get('task-list').children.length,60);
    assert(f.text(f.get('task-list')).includes('Changed externally'));
    assert(f.text(f.get('detail')).includes('Changed externally'));
    assert(f.text(f.get('detail')).includes('版本 2'));
    assert(f.text(f.get('detail')).includes('New next step'));
    assert.equal(f.get('task-list').children[0].attributes['aria-current'],'true');
    assert.equal(f.get('more').hidden,true);
  }finally{await f.get('logout').onclick();}
});
