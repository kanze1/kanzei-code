// Inspect actual source-detail compositing at native and magnified sizes.
async (page) => {
  const origin=await page.evaluate(()=>location.origin);
  const errors=[];
  page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
  page.on('pageerror',e=>errors.push(String(e)));
  await page.route('**/__oc-detail-review',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><html><body style="margin:0;background:#edeae4;color:#303333;font:16px sans-serif"><main id="probe" style="width:768px;height:1152px;position:absolute;left:-10000px"></main><section id="review" style="display:flex;gap:16px;padding:16px"></section></body></html>'}));
  await page.goto(origin+'/__oc-detail-review');
  await page.setViewportSize({width:1240,height:650});
  const result=await page.evaluate(async()=>{
    const {loadOcResources,createOcRenderer}=await import('/22-oc-renderer.js');
    const {createOcDirector}=await import('/22-oc-director.js');
    const {OC_CHARACTER_PACK_SRC}=await import('/22-oc-config.js');
    const resources=await loadOcResources(OC_CHARACTER_PACK_SRC);
    const renderer=createOcRenderer(document.querySelector('#probe'),resources,{export:true});
    renderer.pause();window.detailRenderer=renderer;window.detailResources=resources;
    const base=createOcDirector(resources.pack).sample();
    const clip='replying',time=3.4,track=resources.clips[clip].tracking;
    const frame=Math.floor(time*24),mouth=track.mouth[frame],tattoo=track.tattoo[frame];
    for(const level of [0,.2,.55,1]){
      await renderer.seek({...base,clip,sourceTime:time,to:['replying',frame],previous:null,blend:1},{speaking:level>0,level});
      const column=document.createElement('article');column.innerHTML='<p>嘴型 '+level+'</p>';
      for(const [anchor,span,output] of [[mouth,[.14,.085],[288,174]],[tattoo,[.10,.15],[192,288]]]){
        const canvas=document.createElement('canvas');canvas.width=output[0];canvas.height=output[1];
        const ctx=canvas.getContext('2d');ctx.fillStyle='#edeae4';ctx.fillRect(0,0,...output);
        const w=renderer.canvas.width,h=renderer.canvas.height;
        ctx.drawImage(renderer.canvas,(anchor[0]-span[0]/2)*w,(anchor[1]-span[1]/2)*h,span[0]*w,span[1]*h,0,0,...output);
        column.append(canvas,document.createElement('br'));
      }
      document.getElementById('review').append(column);
    }
    return {canvas:[renderer.canvas.width,renderer.canvas.height],clip,time};
  });
  await page.screenshot({path:'output/playwright/oc-details-closeup.png'});
  if(errors.length)throw new Error(errors.join(';'));
  return {...result,errors};
}
