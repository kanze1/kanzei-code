import http from 'node:http';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import {createHash} from 'node:crypto';

const here=path.dirname(fileURLToPath(import.meta.url));
const root=path.resolve(here,'../../../..');
const js=await readFile(path.join(here,'data.js'),'utf8');
const data=JSON.parse(js.slice(js.indexOf('=')+1).trim().replace(/;$/,''));
const allowed=new Set([...data.files.map(f=>f.path),...data.findings.flatMap(f=>f.refs.map(r=>r[0])),'.kanzei/project/decisions.md']);
const staticFiles=new Map([['/',['index.html','text/html']],['/index.html',['index.html','text/html']],['/style.css',['style.css','text/css']],['/app.js',['app.js','text/javascript']],['/data.js',['data.js','text/javascript']]]);
const server=http.createServer(async(req,res)=>{
  const headers={'Cache-Control':'no-store','X-Content-Type-Options':'nosniff'};
  const send=(status,body,type='text/plain')=>{res.writeHead(status,{...headers,'Content-Type':`${type}; charset=utf-8`});res.end(req.method==='HEAD'?undefined:body);};
  if(!['GET','HEAD'].includes(req.method)){send(405,'Read-only viewer');return;}
  try {
    const url=new URL(req.url,'http://127.0.0.1');
    if(url.pathname==='/favicon.ico'){send(204,'');return;}
    if(url.pathname==='/api/source'){
      const relative=url.searchParams.get('path')||'';
      if(!allowed.has(relative)){send(404,'Source not in audit');return;}
      const resolved=path.resolve(root,relative);
      if(!resolved.startsWith(root+path.sep)){send(404,'Source not in audit');return;}
      const raw=await readFile(resolved);
      const lines=raw.toString('utf8').replace(/^\uFEFF/,'').split(/\r?\n/);
      if(lines.at(-1)==='')lines.pop();
      const requested=Number(url.searchParams.get('line')||1);
      const line=Math.min(lines.length,Math.max(1,Number.isFinite(requested)?Math.floor(requested):1));
      const start=Math.max(1,line-30),end=Math.min(lines.length,start+159);
      send(200,JSON.stringify({path:relative,line,start,end,total:lines.length,text:lines.slice(start-1,end).join('\n'),sha256:createHash('sha256').update(raw).digest('hex')}),'application/json');return;
    }
    const entry=staticFiles.get(url.pathname);
    if(!entry){send(404,'Not found');return;}
    send(200,await readFile(path.join(here,entry[0])),entry[1]);
  } catch {send(500,'Unable to read this audit resource');}
});
const port=Number(process.argv[2]||41739);
server.on('error',error=>{console.error(error.code==='EADDRINUSE'?`Port ${port} is in use. Pass another port to serve.mjs.`:error.message);process.exitCode=1;});
server.listen(port,'127.0.0.1',()=>console.log(`Code map: http://127.0.0.1:${server.address().port}/`));
