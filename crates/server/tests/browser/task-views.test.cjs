const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');
const web=path.join(__dirname,'../../web');
const source=fs.readFileSync(path.join(web,'app.js'),'utf8');
const html=fs.readFileSync(path.join(web,'index.html'),'utf8');
const settle=()=>new Promise(resolve=>setImmediate(resolve));
function fixture(){
  const nodes=new Map(),requests=[];
  function node(){return {children:[],attributes:{},value:'',hidden:false,textContent:'',
    append(...items){this.children.push(...items);},prepend(...items){this.children.unshift(...items);},
    replaceChildren(...items){this.children=items;},setAttribute(k,v){this.attributes[k]=v;},
    querySelectorAll(){return [];},addEventListener(){},close(){},showModal(){}};}
  const get=id=>{if(!nodes.has(id))nodes.set(id,node());return nodes.get(id);};
  const views=[...html.matchAll(/<button data-view="([^"]+)" aria-pressed="([^"]+)">([^<]+)<\/button>/g)].map(([,view,pressed,label])=>Object.assign(node(),{dataset:{view},attributes:{'aria-pressed':pressed},textContent:label}));
  const tasks=['open','in_progress','blocked',...Array(31).fill('closed')].map((status,i)=>({id:i+1,status,title:`0907｜功能｜示例 ${i+1}`,goal:'示例',version:1,closureOutcome:status==='closed'?'completed':null}));
  vm.runInNewContext(source,{
    document:{getElementById:get,createElement:node,querySelectorAll:()=>views},
    location:{hash:''},sessionStorage:{removeItem(){}},URLSearchParams,AbortSignal,AbortController,TextDecoder,setTimeout,clearTimeout,
    fetch:async(url,options)=>{
      requests.push(url);let data;
      if(url==='/api/events')return {ok:true,body:new ReadableStream({start(c){options.signal.addEventListener('abort',()=>c.close(),{once:true});}})};
      if(url==='/api/access')data={role:'reader',local:false};
      else if(url.startsWith('/api/tasks?')){
        const p=new URLSearchParams(url.split('?')[1]);assert(!(p.has('status')&&p.has('view')));
        let filtered=tasks.filter(t=>p.get('status')?t.status===p.get('status'):p.get('view')==='active'?t.status!=='closed':p.get('view')==='in-progress'?t.status==='in_progress':p.get('view')==='blocked'?t.status==='blocked':true);
        if(p.has('query'))filtered=filtered.filter(t=>t.title.includes(p.get('query')));
        const offset=Number(p.get('cursor')||0),size=Number(p.get('pageSize'));const hasMore=filtered.length>offset+size;
        data={tasks:filtered.slice(offset,offset+size),hasMore,nextCursor:hasMore?String(offset+size):null};
      }else if(url.endsWith('/context'))data={task:tasks.find(t=>t.id===Number(url.split('/')[3])),checkpoint:null};
      else if(url.endsWith('/notes'))data={notes:[]};
      else if(url!=='/api/logout')throw Error('Unexpected request '+url);
      return {ok:true,status:200,json:async()=>({ok:true,data})};
    }
  });
  return {get,views,requests,choose:key=>views.find(b=>b.dataset.view===key).onclick(),
    params:()=>new URLSearchParams(requests.filter(r=>r.startsWith('/api/tasks?')).at(-1).split('?')[1]),
    cards:()=>get('task-list').children.filter(n=>n.className?.startsWith('task-card'))};
}
const text=n=>[n.textContent,...n.children.map(text)].join(' ');
test('closed entry preserves default and existing views, pagination, detail and search',async()=>{
  const f=fixture();await settle();
  try{
    assert.equal(f.views.find(v=>v.dataset.view==='closed').textContent,'已关闭');
    assert.equal(f.params().get('view'),'active');assert.equal(f.cards().length,3);
    await f.choose('closed');assert.equal(f.params().get('status'),'closed');assert.equal(f.params().has('view'),false);
    assert.equal(f.views.filter(v=>v.attributes['aria-pressed']==='true')[0].dataset.view,'closed');
    assert.equal(f.cards().length,30);assert(f.cards().every(c=>text(c).includes('已关闭')));assert.equal(f.get('more').hidden,false);
    await f.get('more').onclick();assert.equal(f.params().get('cursor'),'30');assert.equal(f.cards().length,31);assert.equal(f.get('more').hidden,true);
    await f.cards()[0].onclick();assert(text(f.get('detail')).includes('关闭结果'));assert(text(f.get('detail')).includes('completed'));assert(!text(f.get('detail')).includes('编辑任务'));
    f.get('search').value='示例 34';f.get('search-form').onsubmit({preventDefault(){}});await settle();
    assert.equal(f.params().get('status'),'closed');assert.equal(f.params().has('cursor'),false);assert.equal(f.cards().length,1);
    f.get('search').value='no match';f.get('search-form').onsubmit({preventDefault(){}});await settle();assert.equal(f.cards().length,0);assert(text(f.get('task-list')).includes('这里还没有任务'));
    f.get('search').value='';f.get('search-form').onsubmit({preventDefault(){}});await settle();
    await f.get('refresh').onclick();assert.equal(f.params().get('status'),'closed');assert.equal(f.params().has('cursor'),false);
    for(const [view,count]of [['active',3],['in-progress',1],['blocked',1],['recent',30]]){
      await f.choose(view);assert.equal(f.params().get('view'),view);assert.equal(f.params().has('status'),false);assert.equal(f.params().has('cursor'),false);assert.equal(f.cards().length,count);
    }
  }finally{await f.get('logout').onclick();}
});
