/* global window, document */
// Real WebView + IPC + filesystem + provider request, using an owned exact target folder.
import assert from 'node:assert/strict';
import { mkdir,readFile,writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { nativeApp,until,textReply } from './fixtures/native-app.mjs';

assert(process.platform==='win32'&&process.argv[2],'Pass a fresh kzapp.exe');
const output=path.resolve('output/project-init-native');await mkdir(output,{recursive:true});
const description='INIT_CONVERSATION: 我想先讨论该怎么开始，请直接解释，不要登记需求。';
let received=false;
const app=await nativeApp(process.argv[2],output,'项目创建回归',async(body,res)=>{
  if(body.messages?.some(message=>message.role==='user'&&typeof message.content==='string'&&message.content.includes(description)))received=true;
  textReply(res,'INIT_REPLY: 可以直接讨论，需要跟踪时再登记。');
});
const selected=path.join(app.run,'SelectedProject'),checks=[],check=(value,label)=>{assert(value,label);checks.push(label);};
try {
  await mkdir(selected);
  await app.page.evaluate(()=>{document.addEventListener('kz:compose-send',event=>{window.__projectInitSendPrevented=event.defaultPrevented;});});
  await app.page.evaluate(async()=>{(await import('./09-sessions.js')).openNewProjectDialog();});
  await app.page.locator('#new-project-name').fill('SelectedProject');
  await app.page.locator('#new-project-path').fill(selected);
  check(await app.page.locator('#new-project-path').inputValue()===selected,'Explicit project folder keeps its exact full path');
  check((await app.page.locator('#new-project-preview').innerText()).includes(selected),'Preview names the exact selected project folder');
  await app.page.locator('#new-project-git').uncheck();
  await app.page.locator('#new-project-desc').fill(description);
  await app.page.locator('#new-project-create').click();
  await app.page.locator('#new-project-overlay').waitFor({state:'hidden'});
  await until(async()=>await app.page.locator('#prompt').inputValue()===description,'Description becomes an editable draft');
  const created=await app.invoke('projects_get');
  check(path.resolve(created.current)===selected&&existsSync(path.join(selected,'.kanzei')),'Real IPC initializes and registers the selected folder itself');
  check(!existsSync(path.join(selected,'SelectedProject')),'Creation does not produce another same-named folder');
  check(!received,'Creation and description do not start a model request');
  const reqFile=path.join(selected,'.kanzei/project/requirements.md');
  const hasRequirements=async()=>existsSync(reqFile)&&/^##\s+R-\d+/m.test(await readFile(reqFile,'utf8'));
  check(!await hasRequirements(),'Creation and description do not register a requirement');
  await app.page.locator('#prompt').press('Enter');
  await until(()=>received,'First Enter reaches the actual provider as a conversation');
  await until(async()=>await app.page.locator('#prompt').inputValue()===''&&!(await app.invoke('collaboration_snapshot',{projectDir:selected})).some(line=>line.running),'First conversation finishes');
  check(await app.page.evaluate(()=>window.__projectInitSendPrevented===false),'Onboarding does not intercept ordinary composer delivery');
  check(app.requests.some(body=>body.messages?.some(message=>message.role==='user'&&message.content===description)),'First message reaches the provider with its original text');
  check(!await hasRequirements(),'Project requirements stay empty after the first ordinary message');
  check(app.errors.length===0,'Native project workflow has no JavaScript errors');
  await app.page.screenshot({path:path.join(app.run,'project-init.png')});
  await writeFile(path.join(app.run,'acceptance.json'),JSON.stringify({passed:checks.length,checks,selected,executable:app.exe,scope:'Exact owned target folder; real UI handlers, native backend, filesystem and provider request; picker response binding is covered by UI runtime regression'},null,2));
  console.log(`Native project init PASS: ${checks.length} checks; ${app.run}`);
}catch(error){await writeFile(path.join(app.run,'failure.json'),JSON.stringify({error:String(error),checks,errors:app.errors},null,2));throw error;}
finally{await app.close();}
