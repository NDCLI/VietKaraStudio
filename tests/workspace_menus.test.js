const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync(require('node:path').join(__dirname,'../static/studio.js'),'utf8');
function setup(){
  const listeners={};
  const outside={closest:()=>null};
  const menus=Array.from({length:2},()=>{
    const events={};const summary={focused:false,focus(){this.focused=true;},closest:()=>null};
    const button={closest:selector=>selector==='.menu-drop button'?button:null};
    return {open:false,summary,button,events,contains:target=>target===summary||target===button,querySelector:()=>summary,addEventListener:(name,fn)=>events[name]=fn};
  });
  const document={querySelectorAll:()=>menus,addEventListener:(name,fn)=>listeners[name]=fn};
  const context=vm.createContext({document});
  vm.runInContext(source.slice(source.indexOf('function initWorkspaceMenus('),source.indexOf('const SCENE_SECTION_LABELS')),context);
  context.initWorkspaceMenus();
  return {menus,listeners,outside};
}
test('outside pointer closes the menu while a click inside keeps it open',()=>{
  const {menus,listeners,outside}=setup();menus[0].open=true;
  listeners.pointerdown({target:menus[0].summary});assert.equal(menus[0].open,true);
  listeners.pointerdown({target:outside});assert.equal(menus[0].open,false);
});
test('keyboard focus outside dismisses the menu and Escape restores summary focus',()=>{
  const {menus,listeners,outside}=setup();menus[0].open=true;
  listeners.focusin({target:outside});assert.equal(menus[0].open,false);
  menus[0].open=true;listeners.keydown({key:'Escape'});
  assert.equal(menus[0].open,false);assert.equal(menus[0].summary.focused,true);
});
test('switching menus and choosing an action dismisses the old popup',()=>{
  const {menus}=setup();menus[0].open=true;menus[1].open=true;menus[1].events.toggle();
  assert.equal(menus[0].open,false);assert.equal(menus[1].open,true);
  menus[1].events.click({target:menus[1].button});assert.equal(menus[1].open,false);
});
