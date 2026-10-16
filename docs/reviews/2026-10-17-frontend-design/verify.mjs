import http from 'node:http';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {chromium} from 'playwright-core';

const root=path.dirname(fileURLToPath(import.meta.url));
const screenshots=path.resolve(root,'../../..','output/playwright/frontend-design-20261017');
await mkdir(screenshots,{recursive:true});
const types={'.html':'text/html; charset=utf-8','.css':'text/css; charset=utf-8','.js':'text/javascript; charset=utf-8','.png':'image/png','.svg':'image/svg+xml','.json':'application/json; charset=utf-8','.md':'text/plain; charset=utf-8','.txt':'text/plain; charset=utf-8'};
const server=http.createServer(async(req,res)=>{
 const pathname=decodeURIComponent(new URL(req.url,'http://localhost').pathname);
 const file=path.resolve(root,'.'+(pathname==='/'?'/index.html':pathname));
 if(!path.relative(root,file)||path.relative(root,file).startsWith('..')){res.writeHead(403);res.end();return;}
 try {const body=await readFile(file);res.writeHead(200,{'Content-Type':types[path.extname(file)]||'application/octet-stream'});res.end(body);}catch(error){res.writeHead(error.code==='ENOENT'?404:500);res.end(String(error.code));}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const base=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({channel:'msedge',headless:true});
const checks=[],errors=[],resources=[];
const check=(name,pass,evidence)=>{checks.push({name,pass:Boolean(pass),evidence});if(!pass)console.error('FAIL',name,JSON.stringify(evidence));};
const context=await browser.newContext({viewport:{width:1440,height:1000},reducedMotion:'reduce'});
context.on('page',page=>{page.on('pageerror',e=>errors.push(e.message));page.on('response',r=>{if(r.status()>=400)resources.push({url:r.url(),status:r.status()});});});
const page=await context.newPage();
try {
 await page.goto(base+'/round1.html');await page.locator('.finding').last().waitFor();
 check('报告包含全部 10 项发现',await page.locator('.finding').count()===10);
 await page.getByRole('button',{name:'优先处理',exact:true}).click();check('优先处理筛选为 5 项',await page.locator('.finding').count()===5);
 await page.getByRole('button',{name:'全部 10',exact:true}).click();await page.locator('#UI-01 summary').click();check('建议展开包含验收条件',await page.locator('#UI-01').getAttribute('open')!==null&&await page.locator('#UI-01 .accept').isVisible());
 await page.locator('.overview-figure .image-link').click();check('截图可在原生 dialog 放大',await page.locator('#image-viewer').evaluate(e=>e.open));await page.keyboard.press('Escape');check('截图关闭恢复原入口焦点',await page.locator('.overview-figure .image-link').evaluate(e=>e===document.activeElement));
 await page.evaluate(()=>scrollTo({top:0,behavior:'instant'}));await page.screenshot({path:path.join(screenshots,'report-light-1440.png'),fullPage:false});
 await page.getByRole('button',{name:'切换到深色主题',exact:true}).click();check('报告深色主题状态正确',await page.locator('html').getAttribute('data-theme')==='dark');
 const ratios=await page.locator('#color-rows .ratio').allTextContents();check('报告深色状态文字对产品主背景均至少 4.5:1',ratios.every(x=>parseFloat(x)>=4.5),ratios);
 await page.screenshot({path:path.join(screenshots,'report-dark-1440.png')});
 for(const width of [800,420]) {await page.setViewportSize({width,height:1000});await page.goto(base+'/round1.html');const dimensions=await page.evaluate(()=>({client:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth}));check(`报告 ${width}px 无页面横向溢出`,dimensions.scroll<=dimensions.client,dimensions);await page.screenshot({path:path.join(screenshots,`report-light-${width}.png`)});}

 await page.setViewportSize({width:1440,height:1000});await page.goto(base+'/case.html');
 check('案例展示 5 条真实快照',await page.locator('.work-row').count()===5);
 const r344=page.locator('.work-row[data-id="R-344"]');check('批次全部完成仍保留整体进行中',await r344.locator('.status').innerText()==='▶进行中'||(await r344.locator('.status').innerText()).includes('进行中'));
 check('批次数字可见且不依赖悬停',await r344.locator('.progress-text').isVisible()&&(await r344.locator('.progress-text').innerText()).includes('4 / 4'));
 await r344.locator('summary').focus();await page.keyboard.press('Enter');check('键盘能展开详情并读到真实外部验收',await r344.getAttribute('open')!==null&&(await r344.locator('.row-detail').innerText()).includes('现场目标未验收'));
 const focusStyle=await r344.locator('summary').evaluate(e=>({style:getComputedStyle(e).outlineStyle,width:getComputedStyle(e).outlineWidth}));check('键盘焦点轮廓可见',focusStyle.style==='solid'&&parseFloat(focusStyle.width)>=2,focusStyle);
 await page.getByRole('searchbox',{name:'搜索编号或标题'}).fill('R-344');check('搜索精确命中目标',await page.locator('.work-row').count()===1);
 await page.getByLabel('状态',{exact:true}).selectOption('todo');check('联合筛选出现无结果恢复入口',await page.locator('.work-row').count()===0&&await page.getByRole('button',{name:'清除全部筛选',exact:true}).isVisible());
 await page.getByRole('button',{name:'清除全部筛选',exact:true}).click();check('一键清除搜索与状态筛选并恢复焦点',await page.locator('.work-row').count()===5&&await page.locator('#case-search').inputValue()===''&&await page.locator('#case-filter').inputValue()==='all'&&await page.locator('#case-search').evaluate(e=>e===document.activeElement));
 await page.getByRole('button',{name:'调整前',exact:true}).click();check('调整前展示无可见数字的原布局',await page.locator('.legacy-cells').count()===5&&await page.locator('.progress-text').count()===0);
 await page.getByRole('button',{name:'调整后',exact:true}).click();check('调整后恢复明确状态与进度数字',await page.locator('.progress-text').count()===5&&await page.locator('#after').getAttribute('aria-pressed')==='true');
 const p0=await page.locator('.priority[data-priority="P0"]').evaluate(e=>({text:e.textContent,border:getComputedStyle(e).borderTopStyle,weight:getComputedStyle(e).fontWeight}));check('P0 用轮廓和字重单次强化',p0.text==='P0'&&p0.border==='solid'&&Number(p0.weight)>=700,p0);
 await page.getByLabel('灰度检查').check();check('灰度检查实际应用 filter',await page.locator('.case-shell').evaluate(e=>getComputedStyle(e).filter)==='grayscale(1)');await page.getByLabel('灰度检查').uncheck();
 await page.screenshot({path:path.join(screenshots,'case-after-light-1440.png'),fullPage:true});
 await page.getByRole('button',{name:'调整前',exact:true}).click();await page.screenshot({path:path.join(screenshots,'case-before-light-1440.png'),fullPage:true});
 await page.getByRole('button',{name:'调整后',exact:true}).click();await page.getByRole('button',{name:'切换到深色主题',exact:true}).click();await page.screenshot({path:path.join(screenshots,'case-after-dark-1440.png'),fullPage:true});
 for(const width of [800,420]) {await page.setViewportSize({width,height:width===800?500:900});await page.goto(base+'/case.html');const d=await page.evaluate(()=>({client:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth}));check(`案例 ${width}px 无页面横向溢出`,d.scroll<=d.client,d);check(`案例 ${width}px 状态和数字可读`,await page.locator('.status').first().isVisible()&&await page.locator('.progress-text').first().isVisible());if(width===420)check('窄屏隐藏列标题后仍明确标注批次',await page.locator('.progress-label').first().isVisible());await page.screenshot({path:path.join(screenshots,`case-after-light-${width}.png`),fullPage:true});}
 await page.emulateMedia({reducedMotion:'reduce'});const motion=await page.locator('.work-summary').first().evaluate(e=>getComputedStyle(e).transitionDuration);check('尊重 reduced-motion',motion==='0s',motion);
 await page.goto(pathToFileURL(path.join(root,'round1.html')).href);await page.locator('.finding').last().waitFor();check('报告可直接以本地文件打开',await page.locator('.finding').count()===10);
 await page.goto(pathToFileURL(path.join(root,'case.html')).href);await page.locator('.work-row').last().waitFor();check('案例可直接以本地文件打开',await page.locator('.work-row').count()===5);
 check('无浏览器未捕获异常',errors.length===0,errors);check('所有请求的本地资源均正常',resources.length===0,resources);
} catch(error){check('验证脚本完整运行',false,error.stack);} finally {
 await browser.close();await new Promise(resolve=>server.close(resolve));
 const result={date:'2026-10-17',timezone:'Asia/Shanghai',checked_at_utc:new Date().toISOString(),scope:'独立报告与静态交互原型，不是产品原生验收',passed:checks.filter(x=>x.pass).length,total:checks.length,all_pass:checks.every(x=>x.pass),checks,errors,resources,screenshots};
 await writeFile(path.join(root,'qa-results.json'),JSON.stringify(result,null,2)+'\n');
 console.log(JSON.stringify({passed:result.passed,total:result.total,all_pass:result.all_pass,screenshots},null,2));
 if(!result.all_pass)process.exitCode=1;
}
