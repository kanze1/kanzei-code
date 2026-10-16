import http from 'node:http';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright-core';
const root=path.dirname(fileURLToPath(import.meta.url));
const shots=path.resolve(root,'../../..','output/playwright/frontend-design-round2-20261017');await mkdir(shots,{recursive:true});
const types={'.html':'text/html; charset=utf-8','.css':'text/css; charset=utf-8','.js':'text/javascript; charset=utf-8','.svg':'image/svg+xml','.png':'image/png','.json':'application/json; charset=utf-8','.txt':'text/plain; charset=utf-8'};
const server=http.createServer(async(req,res)=>{const url=new URL(req.url,'http://localhost');const file=path.resolve(root,'.'+(url.pathname==='/'?'/index.html':decodeURIComponent(url.pathname)));if(path.relative(root,file).startsWith('..')){res.writeHead(403);res.end();return;}try{const body=await readFile(file);res.writeHead(200,{'Content-Type':types[path.extname(file)]||'text/plain'});res.end(body);}catch(e){res.writeHead(e.code==='ENOENT'?404:500);res.end(String(e.code));}});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const base=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({channel:'msedge',headless:true});const context=await browser.newContext({viewport:{width:1440,height:1000},reducedMotion:'reduce'});const checks=[],errors=[],resources=[];context.on('page',p=>{p.on('pageerror',e=>errors.push(e.message));p.on('response',r=>{if(r.status()>=400)resources.push(r.url());});});
const page=await context.newPage();const check=(name,pass,evidence)=>{checks.push({name,pass:Boolean(pass),evidence});if(!pass)console.log('FAIL',name,evidence);};
try{
 await page.goto(base+'/index.html');await page.locator('#coverage-rows tr').last().waitFor();
 check('补充报告展示 18 条操作路径',await page.locator('#coverage-rows tr').count()===18);
 check('现有主题被明确保留',await page.getByText('当前颜色主题保留。',{exact:false}).isVisible());
 check('5 个点缀候选可查阅',await page.locator('.candidate-list article').count()===5);
 await page.locator('#native [data-image]').first().click();check('原生证据截图可放大',await page.locator('#image-viewer').evaluate(e=>e.open));await page.keyboard.press('Escape');
 await page.evaluate(()=>scrollTo({top:0,behavior:'instant'}));await page.screenshot({path:path.join(shots,'report-round2-light-1440.png')});
 await page.locator('#report-theme').click();check('报告深色主题可切换',await page.locator('html').getAttribute('data-theme')==='dark');
 for(const width of [800,420]){await page.setViewportSize({width,height:950});await page.goto(base+'/index.html');const w=await page.evaluate(()=>({client:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth}));check(`报告 ${width}px 无整页横向溢出`,w.scroll<=w.client,w);await page.screenshot({path:path.join(shots,`report-round2-${width}.png`)});}
 await page.setViewportSize({width:1440,height:950});await page.goto(base+'/cyan-lab.html');
 const baseline=await page.locator('#lab-send').evaluate(e=>({send:getComputedStyle(e).backgroundColor,bg:getComputedStyle(document.body).backgroundColor,accent:getComputedStyle(document.documentElement).getPropertyValue('--accent')}));
 await page.locator('#baseline').click();const old=await page.locator('.completion-glyph').first().evaluate(e=>getComputedStyle(e).color);await page.locator('#accented').click();const changed=await page.locator('.completion-glyph').first().evaluate(e=>getComputedStyle(e).color);
 check('点缀开关确实只改变局部完成图标',old!==changed,{old,changed});
 const after=await page.locator('#lab-send').evaluate(e=>({send:getComputedStyle(e).backgroundColor,bg:getComputedStyle(document.body).backgroundColor,accent:getComputedStyle(document.documentElement).getPropertyValue('--accent')}));check('主题表面与橙色发送按钮保持原值',JSON.stringify(baseline)===JSON.stringify(after),{baseline,after});
 await page.locator('#opt-completion').uncheck();check('可以单独关闭完成点缀',await page.locator('.completion-glyph').first().evaluate(e=>getComputedStyle(e).color)===old);await page.locator('#opt-completion').check();
 await page.locator('#panel-toggle').click();check('面板点击后展开与 aria 同步',await page.locator('#lab-panel').isVisible()&&await page.locator('#panel-toggle').getAttribute('aria-expanded')==='true');await page.locator('#opt-panel').check();check('面板开启时出现可选青蓝细标记',await page.locator('#panel-toggle').evaluate(e=>getComputedStyle(e,'::after').height)==='2px');await page.locator('#panel-close').click();check('关闭后细标记收回并恢复焦点',await page.locator('#lab-panel').isHidden()&&await page.locator('#panel-toggle').evaluate(e=>e===document.activeElement));
 await page.locator('#opt-reference').check();await page.locator('#reference-badge').click();check('引用编号能展开来源',await page.locator('#reference-detail').isVisible());
 await page.locator('#resource-link').click();check('文件资源入口能展开读取结果',await page.locator('#read-result').getAttribute('open')!==null);
 await page.locator('#save-demo').click();check('保存演示后出现明确已保存回执',await page.locator('#saved-feedback').isVisible());
 await page.locator('#lab-send').click();check('运行演示保持橙色且不显示完成标记',await page.locator('#phase-label').innerText()==='运行中'&&await page.locator('.run-receipt').isHidden());await page.locator('#lab-stop').click();check('停止演示恢复输入并收回停止按钮',await page.locator('#phase-label').innerText()==='已停止'&&await page.locator('#lab-stop').isHidden());
 await page.locator('#lab-send').click();await page.locator('#lab-send').click();check('收到完成演示回执后才呈现完成标记',await page.locator('.run-receipt').isVisible()&&await page.locator('#lab-stop').isHidden());
 await page.locator('#opt-panel').uncheck();await page.locator('#opt-reference').uncheck();await page.screenshot({path:path.join(shots,'cyan-light-1440.png'),fullPage:true});
 await page.locator('#lab-theme').click();check('深色点缀使用图标原青蓝',await page.locator('.completion-glyph').first().evaluate(e=>getComputedStyle(e).color)==='rgb(57, 216, 246)');await page.screenshot({path:path.join(shots,'cyan-dark-1440.png'),fullPage:true});
 for(const width of [800,420]){await page.setViewportSize({width,height:900});await page.goto(base+'/cyan-lab.html');const w=await page.evaluate(()=>({client:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth}));check(`点缀原型 ${width}px 无横向溢出`,w.scroll<=w.client,w);await page.screenshot({path:path.join(shots,`cyan-light-${width}.png`),fullPage:true});}
 await page.goto(base+'/implementation.html');
 check('实现报告展示实际管理页面截图',await page.locator('img[src="implementation-assets/management-dark.png"]').count()===1);
 for(const width of [1440,800,420]){await page.setViewportSize({width,height:950});await page.goto(base+'/implementation.html');await page.locator('img[src="implementation-assets/management-dark.png"]').scrollIntoViewIfNeeded();await page.waitForFunction(()=>[...document.images].filter(i=>i.loading!=='lazy').every(i=>i.complete&&i.naturalWidth>0));const w=await page.evaluate(()=>({client:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth}));check(`实现报告 ${width}px 无整页横向溢出`,w.scroll<=w.client,w);await page.screenshot({path:path.join(shots,`implementation-${width}.png`)});}
 check('没有浏览器异常',errors.length===0,errors);check('被请求的本地资源均正常',resources.length===0,resources);
}catch(e){check('验证流程完整运行',false,e.stack);}finally{await browser.close();await new Promise(resolve=>server.close(resolve));const result={date:'2026-10-17',scope:'报告与点缀原型的验证，不代表被审视产品全部检查通过',passed:checks.filter(x=>x.pass).length,total:checks.length,all_pass:checks.every(x=>x.pass),checks,errors,resources,screenshots:shots};await writeFile(path.join(root,'round2/artifact-qa.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({passed:result.passed,total:result.total,all_pass:result.all_pass,screenshots:shots},null,2));if(!result.all_pass)process.exitCode=1;}
