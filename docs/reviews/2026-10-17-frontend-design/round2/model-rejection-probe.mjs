import {chromium} from 'playwright-core';
import {writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.dirname(fileURLToPath(import.meta.url));
const browser=await chromium.launch({channel:'msedge',headless:true});
const page=await browser.newPage({viewport:{width:1440,height:950}});
try{
 await page.goto('http://127.0.0.1:5178/?scene=empty&theme=light');
 await page.waitForFunction(()=>window.__kzPreview?.ready);
 await page.evaluate(()=>window.__kzPreview.setCommand('process_update',()=>{throw '审视注入：写入拒绝';}));
 await page.locator('#model-picker').click();await page.locator('.picker-menu [data-action="show-all"]').click();
 await page.locator('.picker-menu [data-value="ollama:qwen3.5:4b"]').click();
 await page.waitForFunction(()=>document.body.innerText.includes('对话模型保存失败'));
 await page.waitForFunction(()=>document.querySelector('#model-picker').textContent.includes('codex:gpt-5.6-luna')&&!document.querySelector('#model-picker').classList.contains('is-pending'));
 const before=await page.evaluate(async()=>{const s=await import('/03-shell.js');return {chip:document.querySelector('#model-picker').innerText,title:document.querySelector('#model-picker').title,localModel:s.processItems.find(x=>x.id===s.activeProcessId)?.model};});
 await page.evaluate(()=>window.__kzPreview.setCommand('run_prompt',()=>null));
 await page.locator('#prompt').fill('确认显示恢复之后的发送');await page.locator('#send').click();
 await page.waitForFunction(()=>window.__kzPreview.calls.some(c=>c.cmd==='run_prompt'));
 const sent=await page.evaluate(()=>window.__kzPreview.calls.filter(c=>c.cmd==='run_prompt').at(-1).args.model);
 await page.screenshot({path:path.join(root,'assets/browser-16-model-rollback-confirmed.png')});
 const evidence={date:'2026-10-17',scope:'当前前端+受控拒绝回执，不连接真实模型',steps:['选择 ollama:qwen3.5:4b','process_update 返回拒绝','等待按钮恢复 codex:gpt-5.6-luna 且无 pending class','立即发送','记录 run_prompt.model'],before,sent,displayAndSendDisagree:!before.chip.includes(sent),source:['08-models.js:131-164','08-compose-runtime.js:430-433']};
 await writeFile(path.join(root,'model-rejection-evidence.json'),JSON.stringify(evidence,null,2)+'\n');console.log(JSON.stringify(evidence,null,2));
}finally{await browser.close();}
