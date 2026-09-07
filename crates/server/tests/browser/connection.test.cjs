const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const {webcrypto}=require('node:crypto');
const source=fs.readFileSync(require('node:path').join(__dirname,'../../web/app.js'),'utf8');
const key='steward.connection-token';
function fixture(storage=new Map(),detailFails=false,hash='',exchangeFails=false){
  const nodes=new Map(),requests=[];let unauthorized=false;
  const location={hash,pathname:'/',search:''};
  const history={replaceState(_state,_title,url){assert.equal(url,'/');location.hash='';}};
  function node(){return {value:'',hidden:false,disabled:false,textContent:'',append(){},prepend(){},querySelectorAll(){return [];},replaceChildren(){},setAttribute(){},addEventListener(){},close(){},showModal(){}};}
  function get(id){if(!nodes.has(id))nodes.set(id,node());return nodes.get(id);}
  const task={id:1,status:'in_progress',title:'0907｜探索｜Connection',currentSessionId:'trial',version:2};
  vm.runInNewContext(source,{
    document:{getElementById:get,createElement:node,querySelectorAll:()=>[]},
    location,history,
    sessionStorage:{getItem:k=>storage.get(k),setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)},
    // LAN HTTP does not expose randomUUID; getRandomValues is available.
    crypto:{getRandomValues:a=>webcrypto.getRandomValues(a)},Uint8Array,URLSearchParams,AbortSignal,structuredClone,
    fetch:async(path,options)=>{
      requests.push({path,token:options.headers['X-Steward-Token'],code:options.headers['X-Steward-Connect'],hash:location.hash});
      if(path==='/api/connect')return {ok:!exchangeFails,json:async()=>({ok:!exchangeFails,data:exchangeFails?null:{token:'synthetic'}})};
      const status=unauthorized?401:detailFails&&path.endsWith('/context')?500:200;
      const data=path.includes('/context')?{task,checkpoint:null}:path.endsWith('/notes')?{notes:[]}:{tasks:[task],hasMore:false,nextCursor:null};
      return {ok:status===200,status,json:async()=>({ok:status===200,data,error:status===200?null:{code:status===401?'UNAUTHORIZED':'INTERNAL_ERROR',message:'synthetic'}})};
    }
  });
  return {get,requests,storage,location,reject:()=>{unauthorized=true;},login:async()=>{get('credential').value='synthetic';await get('connect-form').onsubmit({preventDefault(){},submitter:node()});}};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
test('one-use fragment is cleared before exchange and exchanged token authenticates queries',async()=>{
  const code='a'.repeat(64),f=fixture(new Map(),false,'#connect='+code);
  await settle();
  assert.equal(f.location.hash,'');
  assert.equal(f.requests[0].path,'/api/connect');assert.equal(f.requests[0].code,code);assert.equal(f.requests[0].hash,'');
  assert(f.requests.slice(1).every(r=>r.token==='synthetic'&&!r.code));
  assert.equal(f.storage.get(key),'synthetic');assert.equal(f.get('workspace').hidden,false);
});
test('failed automatic exchange is not replayed and manual login remains available',async()=>{
  const f=fixture(new Map(),false,'#connect='+'b'.repeat(64),true);await settle();
  assert.equal(f.requests.length,1);assert.equal(f.storage.has(key),false);assert.equal(f.get('workspace').hidden,true);
  assert(f.get('login-error').textContent.includes('自动连接未完成'));
  await f.login();assert.equal(f.get('workspace').hidden,false);assert.equal(f.storage.get(key),'synthetic');
});
test('LAN connection survives search, refresh and reload; logout clears storage',async()=>{
  const f=fixture();await f.login();assert.equal(f.storage.get(key),'synthetic');
  assert.equal(f.get('notice').textContent,'');
  await f.get('refresh').onclick();f.get('search-form').onsubmit({preventDefault(){}});await settle();
  assert(f.requests.every(r=>r.token==='synthetic'));
  const reloaded=fixture(f.storage);await settle();assert.equal(reloaded.get('workspace').hidden,false);
  assert(reloaded.requests.every(r=>r.token==='synthetic'));
  reloaded.get('logout').onclick();assert.equal(f.storage.has(key),false);
});
test('detail failure does not clear authenticated connection',async()=>{
  const f=fixture(new Map(),true);await f.login();await f.get('refresh').onclick();
  assert.equal(f.storage.get(key),'synthetic');assert(f.requests.every(r=>r.token==='synthetic'));
});
test('401 clears stored and in-memory credentials',async()=>{
  const f=fixture();await f.login();f.reject();await f.get('refresh').onclick();
  assert.equal(f.storage.has(key),false);assert.equal(f.get('workspace').hidden,true);
});
