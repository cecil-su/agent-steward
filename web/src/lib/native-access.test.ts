import fs from 'node:fs';
import {afterEach,expect,it,vi} from 'vitest';
const source=fs.readFileSync('../crates/server/web-legacy-readonly/app.js','utf8');
const html=fs.readFileSync('../crates/server/web-legacy-readonly/index.html','utf8');
const row={id:'a'.repeat(64),label:'<img src=x onerror=alert(1)>',peer:'192.0.2.8',agent:'synthetic browser',verificationCode:'AB12CD34',createdAt:1000,expiresAt:2000};
afterEach(()=>{document.body.replaceChildren();window.onpopstate=null;history.replaceState(null,'','/');vi.restoreAllMocks();});
function fixture(admin=false){
  history.replaceState(null,'','/');
  document.body.innerHTML=new DOMParser().parseFromString(html,'text/html').body.innerHTML;
  Object.defineProperty(document,'scrollingElement',{configurable:true,value:document.documentElement});
  for(const dialog of document.querySelectorAll('dialog')){dialog.showModal=()=>dialog.setAttribute('open','');dialog.close=()=>dialog.removeAttribute('open');}
  const transport=vi.fn(async (path:string,_options:RequestInit)=>({ok:true,status:200,json:async()=>({ok:true,data:path==='/api/access-requests'?{pending:[row],grants:[]}:path==='/api/access-request'?{state:'pending',verificationCode:row.verificationCode}:{state:'approved'},warnings:[]})}));
  const end=source.lastIndexOf('  startUiUpdates();');
  const code=source.slice(0,end).replace('(() => {','return (() => {')+`connected=${admin};canManageAccess=${admin};return {requestAccess,loadAccess,decideAccess,resetConnection,pollAccess,api,setReader(){connected=true;canManageAccess=false;$('workspace').hidden=false;}};})();`;
  const api=new Function('document','fetch',code)(document,transport);
  return {...api,transport,writes:()=>transport.mock.calls.filter(([,o])=>o.method==='POST')};
}
it('offers a request button instead of credential inputs and never grants access while pending',async()=>{
  const f=fixture();expect(document.querySelectorAll('#login input')).toHaveLength(0);
  await f.requestAccess();
  expect(f.writes()).toHaveLength(1);expect(f.writes()[0][0]).toBe('/api/access-request');
  expect(document.querySelector('#access-request-state')).toHaveTextContent('等待管理员批准');
  expect(document.querySelector('#access-verification-code')).toHaveTextContent(row.verificationCode);
  expect(document.querySelector('#request-access')).toBeDisabled();
  expect(document.querySelector('#workspace')).toHaveAttribute('hidden');
});
it('renders request metadata as text and only sends the reviewed code once',async()=>{
  const f=fixture(true);await f.loadAccess();
  expect(document.querySelector('#access-pending')).toHaveTextContent(row.label);
  expect(document.querySelector('#access-pending img')).toBeNull();
  await f.decideAccess(row,'approve');
  expect(f.writes()).toHaveLength(1);
  expect(f.writes()[0][0]).toBe(`/api/access-requests/${row.id}/approve`);
  expect(JSON.parse(String(f.writes()[0][1].body))).toEqual({verificationCode:row.verificationCode});
  expect(f.writes()[0][1].headers).toMatchObject({'X-Steward-CSRF':'1','X-Steward-UI-Contract':'5'});
});
it('does not erase database permission warnings during background authorization reads',async()=>{
  const f=fixture(true);
  f.transport.mockImplementationOnce(async()=>({ok:true,status:200,json:async()=>({ok:true,data:{},warnings:[{code:'INSECURE_DATABASE_PERMISSIONS',message:'synthetic permission warning'}]})}));
  await f.api('/api/tasks');await f.loadAccess();
  expect(document.querySelector('#notice')).toHaveTextContent('synthetic permission warning');
});
it('does not allow a reader to approve or revoke access',async()=>{
  const f=fixture();await f.decideAccess(row,'approve');
  await expect(f.api(`/api/access-requests/${row.id}/revoke`,{})).rejects.toThrow('当前界面只读');
  expect(f.writes()).toHaveLength(0);
});
it('holds failed management writes until explicit refresh without automatic retry',async()=>{
  const f=fixture(true);await f.loadAccess();const base=f.transport.getMockImplementation();
  f.transport.mockImplementation((path:string,options:RequestInit)=>options.method==='POST'?Promise.resolve({ok:false,status:409,json:async()=>({ok:false,error:{code:'ACCESS_CHANGED',message:'refresh'}})}):base(path,options));
  await f.decideAccess(row,'approve');await f.loadAccess();await f.decideAccess(row,'approve');
  expect(f.writes()).toHaveLength(1);expect(document.querySelector('#access-approve')).toBeDisabled();
  await f.loadAccess(false,true);expect(document.querySelector('#access-approve')).not.toBeDisabled();
});
it('rechecks reader authorization even without a working SSE stream and clears protected content on 401',async()=>{
  const f=fixture();f.setReader();vi.spyOn(document,'hidden','get').mockReturnValue(false);
  f.transport.mockImplementationOnce(async()=>({ok:false,status:401,json:async()=>({ok:false,error:{code:'UNAUTHORIZED',message:'revoked'}})}));
  await f.pollAccess();
  expect(f.transport.mock.calls[0][0]).toBe('/api/access');
  expect(document.querySelector('#workspace')).toHaveAttribute('hidden');
  expect(document.querySelector('#login')).not.toHaveAttribute('hidden');
  expect(f.writes()).toHaveLength(0);
});
it('discards an administrator response after authorization was cleared',async()=>{
  const f=fixture(true);let finish!:()=>void;
  const base=f.transport.getMockImplementation();const gate=new Promise<void>(resolve=>{finish=resolve;});
  f.transport.mockImplementation(async(path:string,options:RequestInit)=>{await gate;return base(path,options);});
  const loading=f.loadAccess();f.resetConnection();finish();await loading;
  expect(document.querySelector('#access-pending')).toBeEmptyDOMElement();
  expect(document.querySelector('#workspace')).toHaveAttribute('hidden');
});
