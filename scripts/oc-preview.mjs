import http from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root=fileURLToPath(new URL("../crates/kanzei-app/ui/",import.meta.url));
const output=fileURLToPath(new URL(process.argv.includes("--v7")?"../output/oc-film-v7/":process.argv.includes("--v6")?"../output/oc-film-v6/":"../output/oc-film/",import.meta.url));
const types={".html":"text/html; charset=utf-8",".js":"text/javascript",".mjs":"text/javascript",".css":"text/css",".png":"image/png",".json":"application/json",".wav":"audio/wav",".mp4":"video/mp4",".webm":"video/webm"};
const exporting=process.argv.includes("--export");
const server=http.createServer(async(req,res)=>{
  try {
    const url=new URL(req.url,"http://127.0.0.1");
    if(url.pathname==="/favicon.ico"){res.writeHead(204).end();return;}
    if(exporting&&req.method==="POST"&&/^\/__(?:idle_)?frame\/\d{4}$/.test(url.pathname)){
      let length=0;const chunks=[];
      for await(const chunk of req){length+=chunk.length;if(length>6*1024*1024){res.writeHead(413).end();return;}chunks.push(chunk);}
      const folder=url.pathname.startsWith("/__idle_")?"idle-frames":"frames";
      await mkdir(path.join(output,folder),{recursive:true});
      await writeFile(path.join(output,folder,url.pathname.slice(-4)+".png"),Buffer.concat(chunks));
      res.writeHead(204).end();return;
    }
    const file=path.resolve(root,"."+decodeURIComponent(url.pathname==="/"?"/oc-studio.html":url.pathname));
    const relative=path.relative(root,file);
    if(relative.startsWith("..")||path.isAbsolute(relative)){res.writeHead(403).end();return;}
    const data=await readFile(file);
    const headers={"Content-Type":types[path.extname(file)]||"application/octet-stream","Cache-Control":"no-store","Accept-Ranges":"bytes"};
    if(req.headers.range){
      const range=/^bytes=(\d*)-(\d*)$/.exec(req.headers.range);
      let start=range?.[1]?Number(range[1]):0,end=range?.[2]?Number(range[2]):data.length-1;
      if(range&&!range[1]&&range[2]){start=Math.max(0,data.length-Number(range[2]));end=data.length-1;}
      if(!range||(!range[1]&&!range[2])||!Number.isSafeInteger(start)||!Number.isSafeInteger(end)||start<0||start>end||start>=data.length){res.writeHead(416,{...headers,"Content-Range":`bytes */${data.length}`}).end();return;}
      end=Math.min(end,data.length-1);
      res.writeHead(206,{...headers,"Content-Range":`bytes ${start}-${end}/${data.length}`,"Content-Length":end-start+1}).end(data.subarray(start,end+1));return;
    }
    res.writeHead(200,{...headers,"Content-Length":data.length}).end(data);
  } catch {res.writeHead(404).end("Not found");}
});
server.listen(0,"127.0.0.1",()=>console.log("OC studio: http://127.0.0.1:"+server.address().port+"/"));
