const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');
const app=fs.readFileSync(path.join(__dirname,'../../web/app.js'),'utf8');
const updater=app.slice(app.indexOf('  function startUiUpdates(){'),app.indexOf('  const code=new URLSearchParams'));
function fixture(){
  let timer, reloads=0, button, banner, status={packageFormat:1,apiContract:1,release:'new'};
  const nodes={credential:{value:''},search:{value:''},'project-filter':{value:''}};
  const context={modal:null,pendingWrites:0,uncertainWrite:false,query:'',projectFilter:'',
    document:{querySelector:()=>({content:'old'}),body:{prepend:n=>banner=n}},
    $:id=>nodes[id],el:()=>({setAttribute(){},append(){},remove(){banner=null;}}),
    button:(_text,fn)=>{button=fn;return {};},location:{reload:()=>reloads++},
    confirm:()=>true,alert:()=>{},AbortSignal,
    setTimeout:fn=>{timer=fn;},fetch:async()=>({ok:true,json:async()=>status})};
  vm.runInNewContext(updater,context);
  return {context,nodes,tick:()=>timer(),click:()=>button(),status:s=>status=s,reloads:()=>reloads,banner:()=>banner};
}
test('idle page automatically reloads to the adopted UI',async()=>{
  const f=fixture();await f.tick();assert.equal(f.reloads(),1);
});
test('editor preserves input and requires explicit confirmation even after editor closes',async()=>{
  const f=fixture();f.context.modal={};await f.tick();assert(f.banner());assert.equal(f.reloads(),0);
  f.context.modal=null;await f.tick();assert.equal(f.reloads(),0);f.click();assert.equal(f.reloads(),1);
});
test('pending write blocks both automatic and explicitly requested reload',async()=>{
  const f=fixture();f.context.pendingWrites=1;await f.tick();f.click();assert.equal(f.reloads(),0);
  f.context.pendingWrites=0;await f.tick();assert.equal(f.reloads(),0);f.click();assert.equal(f.reloads(),1);
});
test('uncertain write, login input and unapplied search are protected',async()=>{
  for(const mode of ['uncertain','credential','search','project-filter']){
    const f=fixture();if(mode==='uncertain')f.context.uncertainWrite=true;else f.nodes[mode].value='unsaved';
    await f.tick();assert.equal(f.reloads(),0);assert(f.banner());
  }
});
test('rollback to loaded release clears notification; unavailable checks do not reload',async()=>{
  const f=fixture();f.context.modal={};await f.tick();assert(f.banner());
  f.status({packageFormat:1,apiContract:1,release:'old'});await f.tick();assert.equal(f.banner(),null);assert.equal(f.reloads(),0);
  f.context.fetch=async()=>{throw Error('offline');};await f.tick();assert.equal(f.reloads(),0);
});
test('backend contract change may load its new compatible UI, but never auto reload an editor',async()=>{
  const f=fixture();f.status({packageFormat:1,apiContract:2,release:'new'});await f.tick();assert.equal(f.reloads(),1);
  const g=fixture();g.context.modal={};g.status({packageFormat:1,apiContract:2,release:'new'});await g.tick();assert.equal(g.reloads(),0);
});
