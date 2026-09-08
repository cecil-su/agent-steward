// Real-browser project journey, called by smoke.cjs with its synthetic sandbox.
// No default database, provider credentials, real repository, or package install.
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {execFileSync}=require('node:child_process');

module.exports=async function projectSmoke({browser,url,token,readerToken,cli,temp,root}){
  const context=await browser.newContext({viewport:{width:1360,height:1000}});
  const readerContext=await browser.newContext({viewport:{width:390,height:844}});
  let page,viewer,authenticated=false;
  const errors=[];
  async function login(target,credential){
    // Playwright failure call logs can include fill arguments: sanitize auth errors.
    try{
      await target.goto(url);
      await target.getByLabel('本次服务的连接凭据').fill(credential);
      await target.getByRole('button',{name:'连接工作台'}).click();
      await target.locator('#workspace').waitFor({state:'visible'});
    }catch{throw Error('Isolated project browser authentication failed');}
  }
  async function save(command){
    const [response]=await Promise.all([
      page.waitForResponse(r=>new URL(r.url()).pathname==='/api/commands/'+command&&r.request().method()==='POST'),
      page.locator('#submit-action').click(),
    ]);
    const result=await response.json();
    assert.equal(result.ok,true,'project journey command failed: '+command);
    await page.locator('#action-dialog').waitFor({state:'hidden'});
    return result.data;
  }
  const project=ref=>cli('project','show',ref).data.project;
  const task=ref=>cli('task','show',ref).data.task;
  function withoutAssociation(t){
    const copy={...t};for(const key of ['version','updatedAt','projectId','componentIds'])delete copy[key];return copy;
  }
  const sourceBox=(target,id)=>target.locator('#project-detail .info-box').filter({has:target.getByText('源码 #'+id,{exact:true})});
  try{
    page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
    await login(page,token);authenticated=true;
    await page.locator('#projects').click();await page.locator('#create-project').click();
    // Reopen within one browser task, then await the REAL queued close event.
    await page.evaluate(()=>new Promise(resolve=>{
      document.querySelector('#action-dialog').addEventListener('close',()=>resolve(),{once:true});
      document.querySelector('#cancel').click();document.querySelector('#create-project').click();
    }));
    await page.getByLabel('项目名称',{exact:true}).fill('Browser project A');
    const created=(await save('project-create')).project,ref='##'+created.id;
    await page.getByText('尚未登记源码；不影响项目任务管理。',{exact:true}).waitFor();
    assert.equal(created.revision,1);

    // Independent Project CAS: a CLI writer changes only the same sandbox project.
    await page.getByRole('button',{name:'修改项目名称',exact:true}).click();
    await page.getByLabel('项目名称',{exact:true}).fill('Browser project draft');
    cli('project','rename',ref,'--name','External project name','--if-revision',String(project(ref).revision));
    await page.locator('#submit-action').click();
    await page.getByRole('button',{name:'采用此版本，重新审查后提交'}).waitFor();
    assert.equal(await page.getByLabel('项目名称',{exact:true}).inputValue(),'Browser project draft');
    const conflict=JSON.parse(await page.locator('#conflict pre').textContent());
    assert.equal(conflict.id,created.id);assert.equal(conflict.revision,2);
    assert.equal(Object.hasOwn(conflict,'version'),false,'Project conflict must not use a Task snapshot');
    await page.getByRole('button',{name:'采用此版本，重新审查后提交'}).click();
    await save('project-rename');assert.equal(project(ref).name,'Browser project draft');
    await page.locator('#create-project').click();
    await page.getByLabel('项目名称',{exact:true}).fill('BROWSER PROJECT DRAFT');
    await page.locator('#submit-action').click();
    await page.locator('#action-error').filter({hasText:'CONSTRAINT_VIOLATION'}).waitFor();
    await page.locator('#cancel').click();

    await page.getByRole('button',{name:'添加组件',exact:true}).click();
    await page.getByLabel('组件名称',{exact:true}).fill('backend');
    const component=(await save('project-component-add')).component;
    const directory=path.join(temp,'project-directory'),repo=path.join(temp,'project-repo');
    fs.mkdirSync(directory);fs.mkdirSync(repo);
    const body='Synthetic README body must not appear in HTTP navigation.';
    for(const dir of [directory,repo])fs.writeFileSync(path.join(dir,'README.md'),body);
    const git=(...args)=>execFileSync('git',['-C',repo,...args],{encoding:'utf8',stdio:['ignore','pipe','pipe']});
    git('init','-b','main');const gitBefore=git('status','--porcelain=v2','--branch');
    await page.getByRole('button',{name:'登记普通目录',exact:true}).click();
    await page.getByLabel('非 Git 目录绝对路径').fill(directory);
    assert.equal(await page.getByLabel('所属组件（可不选）').inputValue(),'');
    assert.equal(await page.locator('#action-form').evaluate(form=>form.checkValidity()),true,'public source must pass native form validation without a component');
    const plain=(await save('project-source-add')).source;
    assert.equal(plain.componentId,null);
    await page.getByRole('button',{name:'登记 Git 源码',exact:true}).click();
    await page.getByLabel('现有 Git 工作区绝对路径').fill(repo);
    await page.getByLabel('所属组件（可不选）').selectOption('backend');
    const gitSource=(await save('project-source-add')).source;
    assert.equal(gitSource.componentId,component.id);

    await page.locator('#back-tasks').click();await page.locator('#create').click();
    await page.getByLabel('标题（MMDD｜类型｜主题）').fill('0908｜功能｜真实浏览器项目验收');
    const initial=(await save('task-create')).task,taskRef='#'+initial.id;
    assert.equal(initial.projectId,null);
    await page.getByRole('button',{name:'关联项目',exact:true}).click();
    await page.getByLabel('项目（##编号或唯一名称；留空解除）').fill(ref);
    await page.locator('#action-dialog input[type=checkbox]').check();await save('task-project');
    // Hold a real component GET while the user starts a different edit.
    const componentRoute=`**/api/projects/${created.id}/components`;
    let release,started;
    const held=new Promise(resolve=>release=resolve),requested=new Promise(resolve=>started=resolve);
    const delayComponents=async route=>{started();await held;await route.continue();};
    await page.route(componentRoute,delayComponents);
    try{
      await page.getByRole('button',{name:'设置组件范围',exact:true}).click();await requested;
      await page.getByRole('button',{name:'编辑任务',exact:true}).click();
      await page.getByLabel('目标',{exact:true}).fill('Preserve this newer draft');
      const response=page.waitForResponse(r=>new URL(r.url()).pathname===`/api/projects/${created.id}/components`);
      release();await (await response).finished();
      await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
      assert.equal(await page.locator('#action-title').textContent(),'编辑任务');
      assert.equal(await page.getByLabel('目标',{exact:true}).inputValue(),'Preserve this newer draft');
      await page.locator('#cancel').click();
    }finally{release();await page.unroute(componentRoute,delayComponents);}
    await page.getByRole('button',{name:'设置组件范围',exact:true}).click();
    await page.getByLabel('组件名称（每行一项；留空清空）').fill('backend');
    await page.locator('#action-dialog input[type=checkbox]').check();await save('task-components');
    assert.deepEqual(task(taskRef).componentIds,[component.id]);
    assert.deepEqual(withoutAssociation(task(taskRef)),withoutAssociation(initial));
    const other=cli('project','create','--name','Browser project B').data.project;
    cli('task','create','--project','##'+other.id);
    await page.locator('#projects').click();
    const failFilter=async route=>{if(new URL(route.request().url()).searchParams.get('project')===ref)await route.abort('failed');else await route.continue();};
    await page.route('**/api/tasks?*',failFilter);
    try{
      await page.getByRole('button',{name:'查看该项目任务',exact:true}).click();
      await page.locator('#notice').filter({hasText:'无法读取本地服务'}).waitFor();
      assert.equal(await page.locator('#task-list .task-card').count(),0);
      assert.equal(await page.locator('#more').isHidden(),true);
    }finally{await page.unroute('**/api/tasks?*',failFilter);}
    await page.getByRole('button',{name:'筛选项目',exact:true}).click();
    await page.waitForFunction(ref=>document.querySelector('#project-filter').value===ref,ref);
    await page.waitForFunction(()=>document.querySelectorAll('#task-list .task-card').length===1);
    assert((await page.locator('#task-list').textContent()).includes(initial.title));
    await page.locator('#task-list .task-card').click();
    const taskContext=cli('task','context',taskRef).data;
    assert.equal(taskContext.project.id,created.id);
    await page.locator('#projects').click();
    await page.locator('#project-detail h2').filter({hasText:'Browser project draft'}).waitFor();
    const revisionBefore=project(ref).revision,versionBefore=task(taskRef).version;
    let queries=0;page.on('request',request=>{if(new URL(request.url()).pathname===`/api/projects/${created.id}/context`)queries++;});
    await sourceBox(page,gitSource.id).getByRole('button',{name:'查看源码上下文'}).click();
    await page.locator('#submit-action').click();
    assert.equal(await page.locator('#action-form').evaluate(form=>form.checkValidity()),false);
    assert.equal(queries,0,'blank Git worktree must not issue a query');
    await page.getByLabel('此次查询的 Git 工作区绝对路径').fill(repo);
    await page.locator('#submit-action').click();
    await page.getByRole('heading',{name:'项目源码导航',exact:true}).waitFor();
    const navigation=await page.getByLabel('源码目录与文件导航（只读）').inputValue();
    assert(navigation.includes('README.md'));assert(!navigation.includes(body));
    await page.locator('#cancel').click();
    assert.equal(project(ref).revision,revisionBefore);assert.equal(task(taskRef).version,versionBefore);
    assert.equal(git('status','--porcelain=v2','--branch'),gitBefore);
    for(const dir of [directory,repo])assert.equal(fs.readFileSync(path.join(dir,'README.md'),'utf8'),body);
    await page.screenshot({path:path.join(root,'.local/m5-projects-desktop.png'),fullPage:true});
    await page.setViewportSize({width:390,height:844});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'project mobile layout overflows');
    await page.screenshot({path:path.join(root,'.local/m5-projects-mobile.png'),fullPage:true});

    viewer=await readerContext.newPage();viewer.on('pageerror',e=>errors.push(e.message));
    await login(viewer,readerToken);await viewer.locator('#projects').click();
    await viewer.locator('#project-list .task-card').filter({hasText:'Browser project draft'}).click();
    await viewer.locator('#project-detail h2').filter({hasText:'Browser project draft'}).waitFor();
    assert.equal(await viewer.locator('#create-project').isHidden(),true);
    assert.equal(await viewer.getByRole('button',{name:'修改项目名称',exact:true,includeHidden:true}).isHidden(),true);
    await sourceBox(viewer,plain.id).getByRole('button',{name:'查看源码上下文'}).click();
    await viewer.locator('#submit-action').click();
    await viewer.getByRole('heading',{name:'项目源码导航',exact:true}).waitFor();
    assert((await viewer.getByLabel('源码目录与文件导航（只读）').inputValue()).includes('README.md'));
    await viewer.locator('#cancel').click();
    await sourceBox(page,plain.id).getByRole('button',{name:'解除源码登记'}).click();
    await page.locator('#action-dialog input[type=checkbox]').check();await save('project-source-remove');
    assert.equal(fs.readFileSync(path.join(directory,'README.md'),'utf8'),body);
    assert.deepEqual(withoutAssociation(task(taskRef)),withoutAssociation(initial));
    assert.deepEqual(errors,[]);
    console.log('PASS: real project create/CAS/source/task association/filter/context/reader/native validation and desktop/mobile layout');
  }catch(error){
    if(authenticated)await page.screenshot({path:path.join(root,'.local/m5-projects-failure.png'),fullPage:true}).catch(()=>{});
    throw error;
  }finally{await readerContext.close();await context.close();}
};
