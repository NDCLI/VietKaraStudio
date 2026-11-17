const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
function setup({ready=false,available=true,off=false,downloadError=false}={}){
  const elements={};
  for(const id of ['updateStatus','updateAction','autoUpdateEnabled','checkUpdates','updateDialog','updateDialogVersion','updateDialogNotes','updateDialogStatus','updateDialogInstall','updateDialogLater'])elements[id]={textContent:'',disabled:false,hidden:false,open:false,events:{},addEventListener(name,fn){this.events[name]=fn;},showModal(){this.open=true;},close(){this.open=false;}};
  const calls=[],timers=[],settings=new Map(off?[['vietkara_auto_update','off']]:[]);
  const info={supported:true,update_available:available,version:'1.1.17',ready,installed_version:'1.1.16',message:'Status',notes:'Release notes',size:22000000};
  const context=vm.createContext({$:id=>elements[id],state:{job:null,renderJob:null,manualTiming:false},window:{},
    document:{querySelector:()=>null},localStorage:{getItem:k=>settings.get(k),setItem:(k,v)=>settings.set(k,v)},
    setTimeout:fn=>{timers.push(fn);return timers.length;},clearTimeout:()=>{},wrap:fn=>fn,
    saveNow:async()=>{calls.push('save');},handleError:()=>{},
    api:async(url)=>{calls.push(url);if(url.endsWith('/check'))return info;if(url.endsWith('/download')){if(downloadError)throw Error('Network error');return {ready:true,version:'1.1.17'};}return {applying:true};}});
  vm.runInContext(fs.readFileSync(path.join(__dirname,'../static/updates.js'),'utf8'),context);
  context.initUpdates();
  return {context,elements,calls,timers,info};
}
test('startup announces update with no automatic download or installation',async()=>{
  const s=setup();await s.timers.shift()();
  assert.equal(s.elements.updateDialog.open,true);
  assert.match(s.elements.updateDialogVersion.textContent,/1.1.17/);
  assert.deepEqual(s.calls,['/api/updates/check']);
  s.elements.updateDialogLater.onclick();
  assert.equal(s.elements.updateDialog.open,false);
  assert.deepEqual(s.calls,['/api/updates/check']);
});
test('Update downloads, saves the project, then installs once',async()=>{
  const s=setup();await s.timers.shift()();await s.elements.updateDialogInstall.onclick();
  assert.deepEqual(s.calls,['/api/updates/check','/api/updates/download','save','/api/updates/apply']);
  assert.equal(s.elements.updateDialogInstall.disabled,true);
  await s.elements.updateDialogInstall.onclick();
  assert.equal(s.calls.filter(x=>x.endsWith('/apply')).length,1);
});
test('ready update skips downloading and saves before restarting',async()=>{
  const s=setup({ready:true});await s.timers.shift()();await s.elements.updateDialogInstall.onclick();
  assert.deepEqual(s.calls,['/api/updates/check','save','/api/updates/apply']);
});
test('download failure leaves the app open with Retry and Later available',async()=>{
  const s=setup({downloadError:true});await s.timers.shift()();await s.elements.updateDialogInstall.onclick();
  assert.match(s.elements.updateDialogStatus.textContent,/Network error/);
  assert.equal(s.elements.updateDialogInstall.disabled,false);
  assert.equal(s.elements.updateDialogLater.disabled,false);
  assert.equal(s.calls.includes('/api/updates/apply'),false);
});
test('no new release does not show an update dialog',async()=>{
  const s=setup({available:false});await s.timers.shift()();assert.equal(s.elements.updateDialog.open,false);
});
test('busy work postpones the startup prompt and blocks installation',async()=>{
  const s=setup();s.context.state.renderJob={id:'render'};await s.timers.shift()();
  assert.equal(s.elements.updateDialog.open,false);
  await s.elements.updateDialogInstall.onclick();assert.equal(s.calls.includes('/api/updates/download'),false);
  s.context.state.renderJob=null;s.timers.shift()();assert.equal(s.elements.updateDialog.open,true);
});
test('automatic checks honor the existing preference and manual checking remains available',async()=>{
  const s=setup({off:true});assert.equal(s.timers.length,0);
  await s.elements.checkUpdates.onclick();assert.equal(s.elements.updateDialog.open,true);
});
