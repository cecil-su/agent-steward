// Run after cargo build --workspace. Uses only synthetic temporary state.
const {chromium}=require('playwright');
const {spawn,execFileSync}=require('node:child_process');
const fs=require('node:fs');const os=require('node:os');const path=require('node:path');const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..');
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'steward-gui-'));
if(process.platform!=='win32')fs.chmodSync(temp,0o700);
const database=path.join(temp,'test.db');
// Let taskd create and protect this directory using the platform's ACL API.
const runtime=path.join(temp,'runtime');
const daemon=spawn(path.join(root,'target/debug/taskd'),['--database',database,'--runtime-dir',runtime,'--no-open','--require-local-auth','--port','0'],{stdio:['ignore','pipe','pipe']});
let stdout='',stderr='';daemon.stdout.on('data',b=>stdout+=b);daemon.stderr.on('data',b=>stderr+=b);
let browser,page;
const cli=(...args)=>JSON.parse(execFileSync(path.join(root,'target/debug/taskctl'),['--database',database,'--json',...args],{encoding:'utf8'}));
const delay=ms=>new Promise(r=>setTimeout(r,ms));
(async()=>{
 try{
  for(let i=0;i<100&&!stdout.includes('Credential file:');i++){if(daemon.exitCode!==null)throw Error('Daemon startup failed: '+stderr);await delay(50);}
  const url=stdout.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0];const credentialPath=stdout.match(/Credential file: (.+)/)?.[1];assert(url&&credentialPath,'Daemon did not advertise URL and credential path');
  const token=fs.readFileSync(credentialPath,'utf8');assert(!stdout.includes(token));if(process.platform==='win32'){
    for(const target of [runtime,path.dirname(credentialPath),credentialPath]){
      const protectedAcl=execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-Command','if ([System.IO.Directory]::Exists($env:STEWARD_TEST_ACL_PATH)) { [System.IO.Directory]::GetAccessControl($env:STEWARD_TEST_ACL_PATH).AreAccessRulesProtected } else { [System.IO.File]::GetAccessControl($env:STEWARD_TEST_ACL_PATH).AreAccessRulesProtected }'],{encoding:'utf8',env:{...process.env,STEWARD_TEST_ACL_PATH:target}}).trim();
      assert.equal(protectedAcl,'True','credential ACL must be protected');
    }
  }else{assert.equal(fs.statSync(credentialPath).mode&0o777,0o600);}
  browser=await chromium.launch({headless:true, ...(process.env.STEWARD_BROWSER_CHANNEL ? {channel:process.env.STEWARD_BROWSER_CHANNEL}: {})});page=await browser.newPage({viewport:{width:1360,height:1000}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto(url);await page.getByLabel('本次服务的连接凭据').fill(token);await page.getByRole('button',{name:'连接工作台'}).click();await page.locator('#workspace').waitFor({state:'visible'});
  await page.getByRole('button',{name:'新建任务'}).click();await page.getByLabel('标题（MMDD｜类型｜主题）').fill('0907｜功能｜验证跨会话任务交接');await page.getByLabel('目标',{exact:true}).fill('让下一次会话知道当前进展');await page.getByLabel('范围',{exact:true}).fill('本地 CLI、浏览器与临时数据库');await page.getByLabel('验收条件',{exact:true}).fill('可查看任务、保存 Checkpoint 并继续');await page.getByLabel('下一步',{exact:true}).fill('保存第一个 Checkpoint');await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});await page.getByRole('heading',{name:'0907｜功能｜验证跨会话任务交接',exact:true,level:2}).waitFor();
  await page.getByRole('button',{name:'开始任务',exact:true}).click();await page.getByLabel('新执行 Session ID').fill('browser-session-a');await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});
  await page.getByRole('button',{name:'保存 Checkpoint',exact:true}).click();await page.getByLabel('当前进展摘要').fill('完成最小 GUI 流程');await page.getByLabel('已完成（每行一项）').fill('创建任务\n领取 Session');await page.getByLabel('未完成（每行一项）').fill('验证冲突提示');await page.getByLabel('唯一下一步').fill('切换 Session 后继续');await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});
  // A real second writer changes the task while the browser edit form is open.
  await page.getByRole('button',{name:'编辑任务',exact:true}).click();await page.getByLabel('目标',{exact:true}).fill('浏览器未提交的内容必须保留');const current=cli('task','show','1').data.task;cli('task','note','1','--if-version',String(current.version),'--type','progress','--text','另一窗口已记录进展');await page.getByRole('button',{name:'确认提交'}).click();await page.getByRole('button',{name:'采用此版本，重新审查后提交'}).waitFor();assert.equal(await page.getByLabel('目标',{exact:true}).inputValue(),'浏览器未提交的内容必须保留');await page.getByRole('button',{name:'采用此版本，重新审查后提交'}).click();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});
  await page.getByRole('button',{name:'换会话继续',exact:true}).click();await page.getByLabel('新执行 Session ID').fill('browser-session-b');await page.getByRole('checkbox').check();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});
  assert.equal(cli('task','show','1').data.task.currentSessionId,'browser-session-b');
  await page.getByRole('tab',{name:'会话',exact:true}).click();await page.getByText('browser-session-b · 当前执行',{exact:true}).waitFor();
  const currentBox=page.locator('.info-box').filter({has:page.getByText('browser-session-b · 当前执行',{exact:true})});
  await currentBox.getByRole('button',{name:'绑定客户端'}).click();await page.getByLabel('外部 Session ID',{exact:true}).fill('host-browser-b');await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});
  execFileSync(path.join(root,'target/debug/task-hook'),['--database',database,'--session','browser-session-b','--source','generic','--external-session','host-browser-b'],{input:JSON.stringify({eventId:'event-b',kind:'idle',occurredAt:'2026-09-07T00:00:00Z',content:'synthetic private content dropped'}),stdio:['pipe','pipe','pipe']});
  await currentBox.getByRole('button',{name:'查看观测'}).click();await page.getByText('idle · event-b',{exact:true}).waitFor();
  await page.getByRole('button',{name:'清除观测记录',exact:true}).click();await page.getByRole('checkbox').check();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});assert.equal(cli('hook','list','browser-session-b').data.events.length,0);
  const imported=path.join(temp,'reviewed.txt');fs.writeFileSync(imported,'Synthetic reviewed session record.');
  await currentBox.getByRole('button',{name:'导入已审查记录'}).click();await page.getByLabel('本机普通文件绝对路径').fill(imported);await page.getByRole('checkbox').check();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});await currentBox.getByRole('button',{name:'查看导入记录'}).click();await page.getByRole('button',{name:'删除导入副本',exact:true}).click();await page.getByRole('checkbox').check();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});assert.equal(fs.existsSync(imported),true);
  await page.getByRole('button',{name:'记录阻塞',exact:true}).click();await page.getByLabel('阻塞原因').fill('等待验收');await page.getByLabel('恢复条件').fill('验收通过');await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});assert.equal(cli('task','show','1').data.task.status,'blocked');
  await page.getByRole('button',{name:'解除阻塞',exact:true}).click();await page.getByLabel('恢复后的下一步').fill('继续验收');await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});
  // Commit a note and drop the HTTP response. The browser must never replay it.
  const beforeNotes=cli('history','1').data.history.filter(h=>h.changeType==='task.noted').length;
  let posted=0;await page.route('**/api/commands/task-note',async route=>{posted++;await route.fetch();await route.abort('failed');});
  await page.getByRole('button',{name:'记录进展',exact:true}).click();await page.getByLabel('内容',{exact:true}).fill('response lost synthetic');await page.getByRole('button',{name:'确认提交'}).click();await page.getByRole('button',{name:'已核对执行结果，允许再次提交'}).waitFor();assert.equal(posted,1);assert.equal(await page.getByRole('button',{name:'确认提交'}).isDisabled(),true);assert.equal(cli('history','1').data.history.filter(h=>h.changeType==='task.noted').length,beforeNotes+1);await page.getByRole('button',{name:'取消',exact:true}).click();await page.unroute('**/api/commands/task-note');await page.getByRole('button',{name:'刷新',exact:true}).click();
  await page.getByRole('tab',{name:'历史',exact:true}).click();await page.getByText('task.note', {exact:false}).first().waitFor();
  await page.getByRole('tab',{name:'代码现场',exact:true}).click();await page.getByText('未关联 Worktree',{exact:true}).waitFor();
  const repo=path.join(temp,'repo'),worktree=path.join(temp,'worktree');fs.mkdirSync(repo);
  const git=(...args)=>execFileSync('git',['-C',repo,...args],{stdio:'pipe'});git('init','-b','main');git('-c','user.name=Synthetic','-c','user.email=test@example.invalid','commit','--allow-empty','-m','synthetic');git('branch','task-branch');
  await page.getByRole('button',{name:'创建 Worktree',exact:true}).click();await page.getByLabel('Repository 绝对路径').fill(repo);await page.getByLabel('已有本地分支').fill('task-branch');await page.getByLabel('新 Worktree 绝对路径').fill(worktree);await page.getByRole('checkbox').check();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});assert.equal(fs.existsSync(worktree),true);
  fs.writeFileSync(path.join(worktree,'untracked.txt'),'synthetic');await page.getByRole('button',{name:'安全删除 Worktree',exact:true}).click();await page.getByRole('checkbox').check();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-error').filter({hasText:'WORKTREE_SAFETY_REFUSED'}).waitFor();assert.equal(fs.existsSync(worktree),true);await page.getByRole('button',{name:'取消',exact:true}).click();
  fs.unlinkSync(path.join(worktree,'untracked.txt'));await page.getByRole('button',{name:'安全删除 Worktree',exact:true}).click();await page.getByRole('checkbox').check();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});assert.equal(fs.existsSync(worktree),false);
  await page.getByRole('tab',{name:'概览',exact:true}).click();await page.getByText('浏览器未提交的内容必须保留',{exact:true}).waitFor();
  await page.context().grantPermissions(['clipboard-read','clipboard-write']);await page.getByRole('button',{name:'复制交接上下文'}).click();await page.locator('#notice').filter({hasText:'已复制'}).waitFor();assert((await page.evaluate(()=>navigator.clipboard.readText())).includes('browser-session-b'));
  fs.mkdirSync(path.join(root,'.local'),{recursive:true});await page.screenshot({path:path.join(root,'.local/m5-desktop.png'),fullPage:true});await page.setViewportSize({width:390,height:844});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'mobile layout overflows');await page.screenshot({path:path.join(root,'.local/m5-mobile.png'),fullPage:true});
  await page.getByRole('button',{name:'关闭任务',exact:true}).click();await page.getByRole('checkbox').check();await page.getByRole('button',{name:'确认提交'}).click();await page.locator('#action-dialog').waitFor({state:'hidden'});assert.equal(cli('task','show','1').data.task.status,'closed');
  assert.equal(await page.evaluate(()=>localStorage.length),0);assert.equal(await page.evaluate(()=>sessionStorage.length),0);
  const cookies=await page.context().cookies();assert.equal(cookies.length,1);assert.equal(cookies[0].httpOnly,true);assert.equal(cookies[0].sameSite,'Strict');assert.notEqual(cookies[0].value,token);
  const tab=await page.context().newPage();await tab.goto(url);await tab.locator('#workspace').waitFor({state:'visible'});await tab.close();
  await page.reload();await page.locator('#workspace').waitFor({state:'visible'});assert.equal(await page.getByLabel('本次服务的连接凭据').inputValue(),'');
  await page.locator('#logout').click();await page.locator('#login').waitFor({state:'visible'});
  assert.equal(await page.evaluate(()=>sessionStorage.getItem('steward.connection-token')),null);
  await page.reload();await page.locator('#login').waitFor({state:'visible'});assert.equal(await page.locator('#workspace').isHidden(),true);
  // An invalid remembered grant must never regain access.
  await page.context().addCookies([{name:cookies[0].name,value:'synthetic-expired-token',domain:'127.0.0.1',path:'/api',httpOnly:true,sameSite:'Strict'}]);
  const rejected=page.waitForResponse(r=>r.url().includes('/api/tasks')&&r.status()===401);
  await page.reload();await rejected;assert.equal(await page.evaluate(()=>sessionStorage.getItem('steward.connection-token')),null);
  await page.locator('#login').waitFor({state:'visible'});assert.equal(await page.locator('#workspace').isHidden(),true);
  const readerToken=fs.readFileSync(path.join(path.dirname(credentialPath),'readonly-credential'),'utf8');
  const viewer=await browser.newPage();
  try{
    await viewer.goto(url);await viewer.getByLabel('本次服务的连接凭据').fill(readerToken);await viewer.getByRole('button',{name:'连接工作台'}).click();await viewer.locator('#workspace').waitFor({state:'visible'});
    assert.equal(await viewer.locator('#access-role').textContent(),'只读');assert.equal(await viewer.locator('#create').isHidden(),true);
    const denied=await viewer.evaluate(async token=>(await fetch('/api/commands/task-create',{method:'POST',headers:{'X-Steward-Token':token,'Content-Type':'application/json','X-Steward-CSRF':'1'},body:'{"input":{}}'})).status,readerToken);assert.equal(denied,403);
    await viewer.locator('#live-state').filter({hasText:'实时同步'}).waitFor();
    cli('task','create','SSE-EXTERNAL');
    await viewer.locator('.task-card').filter({hasText:'#2'}).waitFor({state:'visible'});
  }finally{await viewer.close();}
  assert.deepEqual(errors,[]);console.log('PASS: browser lifecycle, CAS, Worktree safety, persistent connection, reader write refusal and external CLI SSE updates');
 }catch(error){if(page){await page.screenshot({path:path.join(root,'.local/m5-failure.png'),fullPage:true}).catch(()=>{});console.error('Action error:',await page.locator('#action-error').textContent().catch(()=>''));}throw error;}finally{
  if(page){if(process.exitCode)await page.screenshot({path:path.join(root,'.local/m5-failure.png'),fullPage:true}).catch(()=>{});}
  if(browser)await browser.close();daemon.kill('SIGINT');await delay(300);if(daemon.exitCode===null)daemon.kill('SIGKILL');fs.rmSync(temp,{recursive:true,force:true});
 }
})().catch(e=>{console.error(e);process.exitCode=1;});
