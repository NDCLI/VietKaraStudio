'use strict';
// Pure beat-clock resize logic; only a touching neighbour in the same line can link.
function timelineResize(word,previous,next,kind,delta,linked,duration){
  const minimum=.05,clamp=(x,a,b)=>Math.max(a,Math.min(b,x));
  const result={start:word.start,end:word.end};
  if(kind==='start'){
    const join=linked&&previous&&previous.line===word.line&&Math.abs(word.start-previous.end)<=.06;
    result.start=clamp(word.start+delta,join?Math.max(0,previous.start+minimum):0,word.end-minimum);
    if(join)result.previousEnd=result.start;
  }else if(kind==='end'){
    const join=linked&&next&&next.line===word.line&&Math.abs(next.start-word.end)<=.06;
    result.end=clamp(word.end+delta,word.start+minimum,join?Math.min(duration,next.end-minimum):duration);
    if(join)result.nextStart=result.end;
  }
  return result;
}
function initTimelineEditing(){
  $('timelineLinked').onchange=()=>{
    if(state.job||state.manualTiming)return;
    state.project.settings.timeline_linked=$('timelineLinked').checked;renderTimelineEditing();dirty();
  };
}
function timelineSelectedIndices(){
  return [...state.timelineSelection].filter(i=>Number.isInteger(i)&&i>=0&&i<state.project.words.length);
}
function renderTimelineEditing(){
  if(!state.project)return;
  $('timelineLinked').checked=state.project.settings.timeline_linked!==false;
  $('timelineLinkedLabel').textContent=$('timelineLinked').checked?'Liên kết':'Độc lập';
  const indices=timelineSelectedIndices();
  $('timelineSelectionCount').textContent=indices.length>1?`${indices.length} từ`:'';
}
if(typeof module!=='undefined')module.exports={timelineResize};
