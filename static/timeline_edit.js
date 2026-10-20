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
  const colors=['#e8833a','#e0c341','#8bc34a','#10b981','#14b8a6','#86c8f2','#6366f1','#a855f7','#f6b3ce'];
  $('timelinePalette').innerHTML=colors.map((c,i)=>`<button type="button" class="timeline-swatch" data-label-color="${c}" style="--swatch:${c}" title="Màu nhãn ${i+1} · chọn từ rồi bấm màu" aria-label="Màu nhãn ${i+1}" aria-pressed="false"></button>`).join('')+'<button type="button" class="timeline-swatch clear-swatch" data-label-color="" title="Xóa màu nhãn của từ đang chọn" aria-label="Xóa màu nhãn">×</button>';
  $('timelinePalette').onclick=event=>{
    const button=event.target.closest('[data-label-color]');if(!button||state.job||state.manualTiming)return;
    const selected=timelineSelectedIndices();if(!selected.length)return toast('Chọn từ trên timeline trước. Giữ Ctrl để chọn nhiều từ.');
    remember();for(const i of selected){const w=state.project.words[i];if(button.dataset.labelColor)w.label_color=button.dataset.labelColor;else delete w.label_color;}
    renderWords();drawTimeline();dirty();renderTimelineEditing();
  };
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
  const indices=timelineSelectedIndices(),colors=new Set(indices.map(i=>state.project.words[i].label_color||''));
  document.querySelectorAll('[data-label-color]').forEach(b=>b.setAttribute('aria-pressed',String(indices.length>0&&colors.size===1&&colors.has(b.dataset.labelColor))));
  $('timelineSelectionCount').textContent=indices.length>1?`${indices.length} từ`:'';
}
if(typeof module!=='undefined')module.exports={timelineResize};
