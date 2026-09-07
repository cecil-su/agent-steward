const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const {webcrypto}=require('node:crypto');
const source=fs.readFileSync(require('node:path').join(__dirname,'../../web/app.js'),'utf8');
const settle=()=>new Promise(resolve=>setImmediate(resolve));
function fixture(browser={authorized:false,role:'admin'},hash='',exchangeFails=false){
  const nodes=new Map(),requests=[],storage=new Map([['steward.connection-token','old-secret']]);let eventController;
  const location={hash,pathname:'/',search:''};
  function node(){const handlers=new Map();return {value:'',hidden:false,disabled:false,textContent:'',append(){},prepend(){},querySelectorAll(){return [];},replaceChildren(){},setAttribute(){},addEventListener(n,f){handlers.set(n,f);},close(){handlers.get('close')?.();},showModal(){}};}
  function get(id){if(!nodes.has(id))nodes.set(id,node());return nodes.get(id);}
  const task={id:1,status:'in_progress',title:'0907｜探索｜Connection',currentSessionId:'trial',version:2};
  vm.runInNewContext(source,{
    document:{getElementById:get,createElement:node,querySelectorAll:()=>[]},location,
    history:{replaceState(_s,_t,url){assert.equal(url,'/');location.hash='';}},
    sessionStorage:{removeItem:k=>storage.delete(k)},confirm:()=>true,
    crypto:{getRandomValues:a=>webcrypto.getRandomValues(a)},Uint8Array,URLSearchParams,AbortSignal,AbortController,TextDecoder,setTimeout,clearTimeout,structuredClone,
    fetch:async(path,options)=>{
      requests.push({path,headers:options.headers||{},credentials:options.credentials,hash:location.hash});
      if(path==='/api/connect')browser.authorized=!exchangeFails;
      if(path==='/api/login')browser.authorized=options.headers['X-Steward-Token']==='synthetic';
      let status=browser.authorized||browser.local?200:401;
      if(path==='/api/logout'||path==='/api/browser-sessions/revoke'){browser.authorized=false;status=200;}
      if(path==='/api/events'&&status===200)return {ok:true,status,body:new ReadableStream({start(c){eventController=c;options.signal.addEventListener('abort',()=>{try{c.close();}catch{}},{once:true});}})};
      const data=path==='/api/access'?{role:browser.role,local:browser.local}:path.includes('/context')?{task,checkpoint:null}:path.endsWith('/notes')?{notes:[]}:{tasks:[task],hasMore:false,nextCursor:null};
      return {ok:status===200,status,json:async()=>({ok:status===200,data,error:status===200?null:{code:'UNAUTHORIZED',message:'synthetic'}})};
    }
  });
  return {get,requests,browser,storage,location,change:()=>eventController.enqueue(new TextEncoder().encode('event: changed\ndata: refresh\n\n')),
    login:async()=>{await settle();get('credential').value='synthetic';await get('connect-form').onsubmit({preventDefault(){},submitter:node()});}};
}
test('local ordinary address opens directly without a credential, cookie or connection code',async()=>{
  const f=fixture({authorized:false,role:'admin',local:true});await settle();
  assert.equal(f.get('workspace').hidden,false);assert.equal(f.get('logout').hidden,true);
  assert.equal(f.get('access-role').textContent,'本机 · 管理员');
  assert(f.requests.every(r=>!r.headers['X-Steward-Token']&&r.path!=='/api/connect'&&r.path!=='/api/login'));
  await f.get('revoke-browsers').onclick();assert.equal(f.get('workspace').hidden,false);
  await f.get('logout').onclick(); // Stop the fixture stream via its otherwise hidden handler.
});
test('automatic link exchanges once, clears fragment, and never retains a readable credential',async()=>{
  const f=fixture(undefined,'#connect='+'a'.repeat(64));await settle();
  assert.equal(f.requests[0].path,'/api/connect');assert.equal(f.requests[0].hash,'');assert.equal(f.storage.size,0);
  assert.equal(f.get('workspace').hidden,false);
  assert(f.requests.every(r=>!r.headers['X-Steward-Token']));
  await f.get('logout').onclick();
});
test('ordinary address, reload and a new tab restore a remembered browser grant',async()=>{
  const browser={authorized:true,role:'admin'},a=fixture(browser),b=fixture(browser);await settle();
  assert.equal(a.get('workspace').hidden,false);assert.equal(b.get('workspace').hidden,false);
  assert(a.requests.every(r=>r.credentials==='same-origin'&&!r.headers['X-Steward-Token']));
  await a.get('logout').onclick();await b.get('refresh').onclick();
  assert.equal(b.get('workspace').hidden,true);assert.equal(browser.authorized,false);
});
test('manual login uses credential only for grant creation, and logout sends CSRF header',async()=>{
  const f=fixture();await f.login();assert.equal(f.get('workspace').hidden,false);
  assert.equal(f.requests.filter(r=>r.headers['X-Steward-Token']).length,1);
  assert.equal(f.requests.find(r=>r.headers['X-Steward-Token']).path,'/api/login');
  assert.equal(f.get('credential').value,'');await f.get('logout').onclick();
  assert.equal(f.requests.at(-1).headers['X-Steward-CSRF'],'1');assert.equal(f.browser.authorized,false);
});
test('failed exchange falls back to manual login without replay',async()=>{
  const f=fixture(undefined,'#connect='+'b'.repeat(64),true);await settle();
  assert.equal(f.requests.length,1);assert(f.get('login-error').textContent.includes('自动连接未完成'));
  await f.login();assert.equal(f.get('workspace').hidden,false);await f.get('logout').onclick();
});
test('reader controls remain unavailable and administrator may revoke browser grants',async()=>{
  const f=fixture({authorized:true,role:'reader'});await settle();assert.equal(f.get('create').hidden,true);assert.equal(f.get('revoke-browsers').hidden,true);
  f.get('create').onclick();assert(f.get('notice').textContent.includes('只读'));await f.get('logout').onclick();
  const admin=fixture({authorized:true,role:'admin'});await settle();await admin.get('revoke-browsers').onclick();
  assert.equal(admin.requests.at(-1).path,'/api/browser-sessions/revoke');assert.equal(admin.browser.authorized,false);
});
test('SSE refreshes external changes but preserves open forms',async()=>{
  const f=fixture({authorized:true,role:'admin'});await settle();const count=()=>f.requests.filter(r=>r.path.startsWith('/api/tasks?')).length;
  let before=count();f.change();await new Promise(r=>setTimeout(r,350));assert(count()>before);
  f.get('create').onclick();before=count();f.change();await new Promise(r=>setTimeout(r,350));assert.equal(count(),before);
  f.get('action-dialog').close();await new Promise(r=>setTimeout(r,350));assert(count()>before);await f.get('logout').onclick();
});
