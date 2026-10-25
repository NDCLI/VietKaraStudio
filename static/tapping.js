'use strict';
function openTappingDialog(){
  if(state.job||state.manualTiming)return;
  if(!state.project.beat)throw Error('Hãy nạp beat gốc trước khi căn bằng phím.');
  const tokens=$('lyricsText').value.split(/\s+/).filter(Boolean);
  if(!tokens.length)throw Error('Nhập lời đúng, mỗi câu một dòng, trước khi gõ nhịp.');
  const selected=Math.max(0,Math.min(tokens.length-1,state.selected));
  $('tapStartWord').innerHTML=tokens.map((token,i)=>`<option value="${i}">${i+1}. ${esc(token)}</option>`).join('');
  $('tapStartWord').value=String(selected);
  const same=tokens.length===state.project.words.length&&tokens.every((t,i)=>t===state.project.words[i].text);
  updateTapLeadIn();
  $('tapPrepareNotice').textContent=same?'Chỉ các từ bạn gõ mới thay mốc cũ. Có thể bắt đầu từ từ đang chọn.':'App sẽ chuẩn bị các khối từ theo lời vừa nhập; từ chưa gõ sẽ được đánh dấu ước lượng.';
  $('tapDialog').showModal();
}
function updateTapLeadIn(){
  const i=Number($('tapStartWord').value)||0,w=state.project.words[i];
  const tokens=$('lyricsText').value.split(/\s+/).filter(Boolean);
  const matched=tokens.length===state.project.words.length&&tokens.every((t,j)=>t===state.project.words[j].text);
  const at=matched&&w?mapTime(w.start):0;
  $('tapStartTime').value=Math.max(0,at-15).toFixed(2);
  $('tapLeadIn').textContent=matched&&w?`Từ đã chọn ở ${time(at,true)} · phát sớm 15 giây (tối thiểu 00:00).`:'Chưa có mốc từ: phát từ đầu beat.';
}
function timingLocks(active){
  document.querySelectorAll('[data-label-color]').forEach(b=>b.disabled=active||!!state.job);
  document.body.classList.toggle('is-tapping',active);
  for(const id of ['timelineLinked','timelineAutoBtn','timelineManualBtn','newBtn','projectsBtn','exportBtn','analyzeBtn','alignLyricsBtn','estimateBtn','listenTrack','applyWordBtn','deleteWordBtn','splitLineBtn','mergeLineBtn','addAnchorBtn','alternateRolesBtn','autoRolesBtn','swapRolesBtn','lyricsText','undoBtn','importSrtBtn','referenceInput','beatInput','backgroundInput','countdownInput','logoInput','videoReferenceBtn','videoSeparateBtn'])$(id).disabled=active||!!state.job;
}
async function beginTapping(){
  if(state.job||state.manualTiming)return;
  const text=$('lyricsText').value.trim(),tokens=text.split(/\s+/).filter(Boolean),start=Number($('tapStartTime').value);
  if(!tokens.length||!Number.isFinite(start)||start<0||start>=state.project.beat.duration)throw Error('Kiểm tra lời và vị trí bắt đầu trong beat.');
  const index=Math.max(0,Math.min(tokens.length-1,Number($('tapStartWord').value)||0));
  remember(textFromWords()||text);await saveNow();
  if(tokens.length!==state.project.words.length||!tokens.every((t,i)=>t===state.project.words[i].text)){
    state.project=await api(endpoint('/estimate'),{method:'POST',body:{text}});state.layout=null;renderAll();
  }
  // Measure a stable layout in lyric order. Partial new timestamps must not reorder
  // words or move lines while the user is still tapping a phrase.
  const plan=await api('/api/layout',{method:'POST',body:{settings:state.project.settings,
    words:state.project.words.map((w,i)=>({...w,start:i*.5+1,end:i*.5+1.4})),anchors:[],beat:null}});
  const clock=new TapTiming(state.project.words.length,index);
  state.manualTiming={clock,plan,startIndex:index,changed:0,before:new Map(),preview:new Map()};
  $('tapDialog').close();timingLocks(true);$('manualTimingBar').hidden=false;
  setAudio('beat',start);$('playBtn').focus();updateTappingCue();
  try{await audio.play();}catch(error){await endTapping(false);throw error;}
}
function acceptTap(item){
  if(!item)return;
  const m=state.manualTiming,word=state.project.words[item.index],start=mapTime(item.start,true),end=mapTime(item.end,true);
  if(!word||start<0||end<=start){
    // Restore the beat-clock cursor so the user can fix anchors and retry this word.
    m.clock.history.pop();m.clock.index=item.index;m.clock.cancel();
    throw Error('Mốc đồng bộ hiện tại đưa từ này ra trước bản hát. Hãy sửa mốc hoặc chọn vị trí beat muộn hơn.');
  }
  m.before.set(item.index,{start:word.start,end:word.end,estimated:word.estimated,manual:word.manual});
  word.start=start;word.end=end;word.estimated=false;word.manual=true;m.changed++;
  state.selected=item.index;renderWords();renderSelected();drawTimeline();dirty();
  if(m.changed%5===0)saveNow().catch(handleError);
}
function updateTappingCue(){
  const m=state.manualTiming;if(!m)return;
  const clock=m.clock,words=state.project.words,next=words[clock.index];
  $('tapProgress').textContent=`${Math.min(clock.index+1,words.length)} / ${words.length}`;
  const cue=$('tapCurrentWord'),text=next?.text||'Đã hết lời';
  if(cue.textContent!==text){
    cue.textContent=text;
    if(!matchMedia('(prefers-reduced-motion: reduce)').matches){
      cue.getAnimations().forEach(a=>a.cancel());
      cue.animate([{opacity:.5,transform:'translateY(4px)'},{opacity:1,transform:'translateY(0)'}],{duration:120,easing:'ease-out'});
    }
  }
  $('tapContext').textContent=words.slice(clock.index+1,clock.index+5).map(w=>w.text).join(' ');
  $('manualTimingBar').classList.toggle('holding',clock.pressing);
  $('tapKeyHint').textContent=audio.paused?'Bấm Phát để tiếp tục nghe beat.':clock.pressing?'Đang giữ Space · thả khi hết từ.':'Gõ nhanh để bắt nhịp từ; giữ–thả để đặt đầu và cuối từ.';
}
async function endTapping(finalize=true){
  const m=state.manualTiming;if(!m)return;
  if(finalize)acceptTap(m.clock.finish(beatNow()));
  state.manualTiming=null;audio.pause();timingLocks(false);$('manualTimingBar').hidden=true;
  await saveNow();renderWords();renderSelected();
  toast(`Đã lưu nhịp ${m.changed} từ. ${m.clock.done?'Đã căn hết lời.':'Các từ chưa gõ giữ mốc cũ hoặc mốc ước lượng.'}`);
}
function previousTap(){
  const m=state.manualTiming;if(!m)return;
  const item=m.clock.back();if(item){
    const word=state.project.words[item.index],before=m.before.get(item.index);
    if(before){word.start=before.start;word.end=before.end;word.estimated=before.estimated;if(before.manual==null)delete word.manual;else word.manual=before.manual;}
    m.before.delete(item.index);m.changed=Math.max(0,m.changed-1);state.selected=item.index;
    m.preview.clear();
    audio.currentTime=Math.max(0,item.start-1);
    renderWords();renderSelected();drawTimeline();dirty();
  }
  updateTappingCue();
}
function initTapping(){
  $('estimateBtn').onclick=wrap(openTappingDialog);
  $('tapStartWord').onchange=updateTapLeadIn;
  $('beginTapBtn').onclick=wrap(async()=>{
    if($('beginTapBtn').disabled)return;
    $('beginTapBtn').disabled=true;
    try{await beginTapping();}finally{$('beginTapBtn').disabled=false;}
  });
  $('finishTapBtn').onclick=wrap(()=>endTapping(true));
  $('stopTapBtn').onclick=wrap(()=>endTapping(false));
  document.addEventListener('keydown',event=>{
    const m=state.manualTiming;if(!m)return;
    if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='z'){
      event.preventDefault();event.stopImmediatePropagation();endTapping(false).then(undo).catch(handleError);return;
    }
    if(!['Space','Enter','Escape','Backspace'].includes(event.code))return;
    event.preventDefault();event.stopImmediatePropagation();
    try{
      if(event.code==='Escape'){endTapping(false).catch(handleError);return;}
      if(event.code==='Enter'){endTapping(true).catch(handleError);return;}
      if(event.code==='Backspace'){if(!event.repeat)previousTap();return;}
      if(audio.paused){toast('Bấm nút Phát để nghe beat rồi gõ nhịp.');return;}
      acceptTap(m.clock.down(beatNow(),event.repeat,event.timeStamp));updateTappingCue();
      if(m.clock.done)endTapping(false).catch(handleError);
    }catch(error){handleError(error);}
  },true);
  document.addEventListener('keyup',event=>{
    const m=state.manualTiming;if(!m||event.code!=='Space')return;
    event.preventDefault();event.stopImmediatePropagation();
    try{acceptTap(m.clock.up(beatNow(),event.timeStamp));updateTappingCue();if(m.clock.done)endTapping(false).catch(handleError);}
    catch(error){handleError(error);}
  },true);
  audio.addEventListener('ended',()=>{if(state.manualTiming)endTapping(true).catch(handleError);});
  audio.addEventListener('play',updateTappingCue);
  audio.addEventListener('pause',()=>{if(state.manualTiming&&!audio.ended)state.manualTiming.clock.cancel();updateTappingCue();});
  audio.addEventListener('seeking',()=>{if(state.manualTiming)state.manualTiming.clock.cancel();updateTappingCue();});
  window.addEventListener('blur',()=>{const m=state.manualTiming;if(m&&m.clock.armed!=null){m.clock.cancel();audio.pause();updateTappingCue();}});
}

function tappingRows(plan){
  const m=state.manualTiming,clock=m.clock;
  const active=clock.armed!=null?clock.index:clock.history.at(-1)?.index??clock.index;
  const row=plan.rows.find(r=>r.words.some(w=>w.source_index===active));
  if(!row)return [];
  const following=plan.rows.slice(row.index+1).find(r=>r.slot!==row.slot);
  const other=following||plan.rows.slice(0,row.index).reverse().find(r=>r.slot!==row.slot);
  return [row,other].filter(Boolean).map(r=>({...r,opacity:1}));
}
function tappingProgress(index,t,word,normal){
  const m=state.manualTiming;if(!m)return normal;
  const durations=m.clock.history.slice(-8).map(w=>w.end-w.start).filter(d=>d>=.12&&d<=1.5).sort((a,b)=>a-b);
  const expected=durations.length?durations[Math.floor(durations.length/2)]:Math.max(.22,Math.min(1.2,word.end-word.start));
  const target=index<m.startIndex?1:tapPreviewTarget(m.clock,m.before,index,t,expected);
  const now=performance.now()/1000,previous=m.preview.get(index)||{value:target,at:now};
  const value=smoothTapProgress(previous.value,target,now-previous.at);
  m.preview.set(index,{value,at:now});return value;
}
