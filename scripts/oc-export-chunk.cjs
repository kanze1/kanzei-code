async (page) => page.evaluate(async () => {
  const first=window.ocExportCursor||0;
  const result=await window.ocStudio.exportFrames(first,120,window.ocExportMode||"demo");
  window.ocExportCursor=first+result.count;
  return {...result,mode:window.ocExportMode||"demo"};
})
