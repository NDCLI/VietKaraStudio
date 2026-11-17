'use strict';
// Enhance the native select's label slot; values, options and events stay native.
(()=>{
  if(!CSS.supports('appearance','base-select'))return;
  const observed=new WeakSet();
  const observer=new MutationObserver(records=>{
    for(const record of records){
      if(record.target instanceof HTMLSelectElement)upgrade(record.target);
      for(const node of record.addedNodes){
        if(!(node instanceof Element))continue;
        if(node instanceof HTMLSelectElement)upgrade(node);
        node.querySelectorAll('select').forEach(upgrade);
      }
    }
  });
  function upgrade(select){
    if(select.multiple||select.hasAttribute('size'))return;
    if(!observed.has(select)){observer.observe(select,{childList:true});observed.add(select);}
    if(select.firstElementChild?.matches('button[data-select-trigger]'))return;
    const trigger=document.createElement('button');
    trigger.type='button';trigger.dataset.selectTrigger='';
    trigger.append(document.createElement('selectedcontent'));
    select.prepend(trigger);
  }
  document.querySelectorAll('select').forEach(upgrade);
  // Scene controls are the one editor region that creates selects dynamically.
  const scenes=document.getElementById('sceneList');
  if(scenes)observer.observe(scenes,{childList:true,subtree:true});
})();
