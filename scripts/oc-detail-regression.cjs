// Pixel checks for single-mouth rendering and detail-layer isolation.
async(page)=>{
  const origin=await page.evaluate(()=>location.origin),errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
  await page.route('**/__oc-detail-check',route=>route.fulfill({contentType:'text/html',body:'<body><div id="host" style="width:768px;height:1152px"></div></body>'}));
  await page.goto(origin+'/__oc-detail-check');
  const result=await page.evaluate(async()=>{
    const {loadOcResources,createOcRenderer}=await import('/22-oc-renderer.js');
    const {createOcDirector}=await import('/22-oc-director.js');
    const {OC_CHARACTER_PACK_SRC}=await import('/22-oc-config.js');
    const resources=await loadOcResources(OC_CHARACTER_PACK_SRC);
    if(resources.pack.mouth.closed!=='clean-plate')throw new Error('Missing clean detail plates');
    const renderer=createOcRenderer(document.getElementById('host'),resources,{export:true});renderer.pause();
    // Component thresholds use a fixed inspection size across desktop DPI
    // settings; native WebView2 can draw this host at a higher resolution.
    const reader=document.createElement('canvas');reader.width=768;reader.height=1152;
    const ctx=reader.getContext('2d',{willReadFrequently:true}),w=reader.width,h=reader.height;
    const base=createOcDirector(resources.pack).sample();let positions=0,levels=0,maxOutsideChanges=0;
    const capture=()=>{ctx.clearRect(0,0,w,h);ctx.drawImage(renderer.canvas,0,0,w,h);return ctx.getImageData(0,0,w,h).data;};
    function mouthComponents(data,track){
      const cx=Math.round(track[0]*w),cy=Math.round(track[1]*h),rw=61,rh=45,mask=new Uint8Array(rw*rh);
      const luminance=(x,y)=>{const i=(y*w+x)*4;return .2126*data[i]+.7152*data[i+1]+.0722*data[i+2];};
      const skin=(luminance(cx-24,cy-8)+luminance(cx+24,cy-8)+luminance(cx-24,cy+8)+luminance(cx+24,cy+8))/4;
      // Low-contrast lip fill connects the darker corners after DPI resampling.
      // Only components containing strong ink count as a separate mouth.
      for(let y=0;y<rh;y++)for(let x=0;x<rw;x++){
        const ink=skin-luminance(cx+x-30,cy+y-22);
        mask[y*rw+x]=((x-30)/20)**2+((y-22)/9)**2<1?(ink>33?2:ink>12?1:0):0;
      }
      const areas=[];
      for(let i=0;i<mask.length;i++)if(mask[i]){
        let area=mask[i]===2?1:0;const stack=[i];mask[i]=0;
        while(stack.length){const p=stack.pop(),x=p%rw,y=Math.floor(p/rw);
          for(const [dx,dy] of [[1,0],[-1,0],[0,1],[0,-1],[1,1],[-1,1],[1,-1],[-1,-1]]){
            const nx=x+dx,ny=y+dy,q=ny*rw+nx;if(nx>=0&&nx<rw&&ny>=0&&ny<rh&&mask[q]){if(mask[q]===2)area++;mask[q]=0;stack.push(q);}
          }
        }if(area>4)areas.push(area);
      }return areas;
    }
    try{
      for(const [id,clip] of Object.entries(resources.pack.clips))for(const time of [clip.start,(clip.start+clip.end)/2,clip.end-1/24]){
        const index=Math.floor(time*24+.001),track=resources.clips[id].tracking.mouth[index];
        const sample={...base,clip:id,sourceTime:time,previous:null,blend:1,to:[base.state,index]};
        await renderer.seek(sample,{speaking:false,level:0});const closed=capture();positions++;
        for(const level of [.12,.2,.55,1]){
          await renderer.seek(sample,{speaking:true,level});const current=capture();let outside=0;
          for(let y=0;y<h;y++)for(let x=0;x<w;x++){
            if(Math.abs(x-track[0]*w)<w*.038&&Math.abs(y-track[1]*h)<h*.026)continue;
            const at=(y*w+x)*4;if(Math.abs(current[at]-closed[at])+Math.abs(current[at+1]-closed[at+1])+Math.abs(current[at+2]-closed[at+2])>3)outside++;
          }
          maxOutsideChanges=Math.max(maxOutsideChanges,outside);
          if(outside)throw new Error(id+' at '+time+' level '+level+' mouth changed surrounding pixels: '+outside+' '+JSON.stringify(renderer.snapshot()));
          if(level>=.2){const components=mouthComponents(current,track);if(components.length!==1)throw new Error(id+' at '+time+' level '+level+' has '+JSON.stringify(components)+' mouth components');}
          levels++;
        }
      }
    }finally{renderer.destroy();}
    return {positions,levels,maxOutsideChanges,checks:['single mouth at partial and full opening','face and tattoo unchanged by speech level','all clip endpoints and midpoints']};
  });
  if(errors.length)throw new Error(errors.join(';'));
  return {passed:true,...result,errors};
}
