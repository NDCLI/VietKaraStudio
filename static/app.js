'use strict';
const $ = id => document.getElementById(id);
const audio = $('audio');
const state = {project:null, selected:-1, timelineSelection:new Set(), job:null, renderJob:null, manualTiming:null, pendingRoles:null, followPlayhead:true, track:'beat', zoom:1, pan:0, undo:[], redo:[],
  saveTimer:null, saveChain:Promise.resolve(), rectangles:[], drag:null, toastTimer:null, presets:[], presetId:null, layout:null, showWaves:true};
const esc = text => String(text).replace(/[&<>"']/g, c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const AI_MODEL_STORAGE='vietkara_ai_model',AI_LANGUAGE_STORAGE='vietkara_ai_language';
function selectedAiModel(){return localStorage.getItem(AI_MODEL_STORAGE)||'medium';}
function selectedAiLanguage(){return localStorage.getItem(AI_LANGUAGE_STORAGE)||'vi';}
function updateAiButton(){const model=selectedAiModel(),label=model==='small'?'small':model==='large-v3-turbo'?'large-v3-turbo':'medium';$('aiModelBtn').textContent='✦ AI · Whisper '+label;}
function sendAnalyze(lyrics=''){return api(endpoint('/analyze'),{method:'POST',body:{model:selectedAiModel(),language:selectedAiLanguage(),device:'auto',...(lyrics?{lyrics}: {})}});}

async function api(path, options={}) {
  const opts={...options};
  if(opts.body && !(opts.body instanceof FormData)) {
    opts.headers={...opts.headers,'Content-Type':'application/json'};
    opts.body=JSON.stringify(opts.body);
  }
  const response=await fetch(path,opts);
  const result=await response.json();
  if(!response.ok) throw Error(result.error || `Lỗi ${response.status}`);
  return result;
}
function endpoint(suffix=''){return `/api/projects/${state.project.id}${suffix}`;}
function toast(message,error=false){clearTimeout(state.toastTimer);$('toast').textContent=message;$('toast').classList.toggle('error',error);$('toast').hidden=false;state.toastTimer=setTimeout(()=>$('toast').hidden=true,error?12000:5500);}
function handleError(error){if(error?.name==='AbortError'||/interrupted by a/i.test(String(error?.message||error)))return;console.error(error);toast(error.message||String(error),true);}
function wrap(fn){return (...args)=>{try{return Promise.resolve(fn(...args)).catch(handleError);}catch(error){handleError(error);}};}
function time(t,precise=false){t=Math.max(0,Number(t)||0);const cs=Math.round(t*100),m=Math.floor(cs/6000),s=Math.floor(cs/100)%60;return `${String(m).padStart(2,'0')}:${String(s).padStart(2,'0')}${precise?'.'+String(cs%100).padStart(2,'0'):''}`;}
function mapTime(t,inverse=false){const anchors=state.project?.anchors||[];if(!anchors.length)return t;const x=inverse?'beat':'ref',y=inverse?'ref':'beat';if(anchors.length===1)return t+anchors[0][y]-anchors[0][x];let pair=anchors.slice(0,2);for(let i=0;i<anchors.length-1;i++){pair=anchors.slice(i,i+2);if(t<=pair[1][x])break;}return pair[0][y]+(t-pair[0][x])*(pair[1][y]-pair[0][y])/(pair[1][x]-pair[0][x]);}
function wordOnBeat(word){return {...word,start:mapTime(word.start),end:mapTime(word.end)};}
function beatDuration(){return state.project?.beat?.duration||state.project?.reference?.duration||1;}
function beatNow(){return state.track==='beat'?audio.currentTime:mapTime(audio.currentTime);}
function trackDuration(){return state.track==='vocals'?(state.project?.vocals?.duration||state.project?.reference?.duration):state.project?.[state.track]?.duration;}
function historySnapshot(text=$('lyricsText').value){return JSON.stringify({words:state.project.words,anchors:state.project.anchors,text,settings:state.project.settings,name:state.project.name,scenes:state.project.scenes||[]});}
function remember(previousText=$('lyricsText').value){state.undo.push(historySnapshot(previousText));if(state.undo.length>60)state.undo.shift();state.redo=[];updateHistoryButtons();}
function updateHistoryButtons(){$('undoBtn').disabled=!state.undo.length;$('redoBtn').disabled=!state.redo.length;}
function restoreHistorySnapshot(snapshot){const value=JSON.parse(snapshot);state.project.words=value.words;state.project.anchors=value.anchors;state.project.settings=value.settings||state.project.settings;state.project.name=value.name||state.project.name;state.project.scenes=value.scenes||[];if(value.text!=null){state.project.text=value.text;$('lyricsText').value=value.text;}$('projectName').value=state.project.name;$('previewTitle').textContent=state.project.name.toUpperCase();state.selected=-1;state.timelineSelection.clear();renderAnchors();renderWords();renderSelected();renderStyleControls();renderSceneControls();renderStyle();drawTimeline();dirty();updateHistoryButtons();}
function selectedWord(){return state.project?.words[state.selected];}
function dirty(){$('renderResult').hidden=true;if(state.job)return;$('saveStatus').textContent='Đang lưu…';clearTimeout(state.saveTimer);state.saveTimer=setTimeout(()=>saveNow().catch(handleError),550);}
async function saveNow(){clearTimeout(state.saveTimer);state.saveTimer=null;if(!state.project||state.job)return;const pid=state.project.id;const payload={name:$('projectName').value,text:$('lyricsText').value,words:state.project.words,anchors:state.project.anchors,settings:state.project.settings,scenes:state.project.scenes||[]};const snapshot=JSON.parse(JSON.stringify(payload));const next=state.saveChain.catch(()=>{}).then(()=>api(`/api/projects/${pid}`,{method:'PATCH',body:snapshot}));state.saveChain=next;try{const updated=await next;if(state.project.id===pid&&state.saveChain===next){$('saveStatus').textContent='Đã lưu trên máy';refreshBackingStatus(updated);}}catch(error){$('saveStatus').textContent='Chưa lưu được';throw error;}}
async function openProject(pid){if(state.renderJob&&state.project?.id!==pid)return toast('Đang dựng snapshot của project này. Hãy đợi xong trước khi chuyển project.');await saveNow();audio.pause();state.backingListening=false;state.titlePreviewAt=null;state.project=await api(`/api/projects/${pid}`);state.layout=null;state.selected=-1;state.timelineSelection.clear();state.undo=[];state.redo=[];updateHistoryButtons();state.pan=0;$('pan').value=0;$('renderResult').hidden=true;localStorage.setItem('obs_project',pid);renderAll();setAudio(state.project.beat?'beat':'reference',0);const missing=Object.keys(state.project.missing_media||{});if(missing.length)toast(`Project thiếu media (${missing.join(', ')}). Nạp lại file ở mục nguồn; dấu vân tay đã lưu trong project.`);if(state.project.interrupted_job){const names={analyze:'nhận lời / căn timing',render:'dựng video',separate:'tách vocal / beat',backing:'tách giọng bè',upload:'nạp media',model:'tải model','align-region':'căn lại đoạn','sync-suggest':'dò đồng bộ','roles':'gợi ý vai hát'};toast(`Tác vụ ${names[state.project.interrupted_job.kind]||'xử lý'} bị gián đoạn khi ứng dụng đóng. Project vẫn giữ nguyên; hãy chạy lại bước đó.`);}if(state.project.job)followJob(state.project.job);}
async function newProject(){if(state.renderJob)return toast('Đang dựng snapshot của project này. Hãy đợi xong trước khi chuyển project.');await saveNow();const project=await api('/api/projects',{method:'POST',body:{}});state.project=null;await openProject(project.id);}
function renderAll(){const p=state.project;if(!p)return;refreshBackingStatus(p);renderWaveTrackControls();$('projectName').value=p.name;$('previewTitle').textContent=p.name.toUpperCase();$('lyricsText').value=p.text||'';for(const slot of ['reference','beat']){$(`${slot}Name`).textContent=p[slot]?p[slot].name:p.missing_media?.[slot]?.name?`${p.missing_media[slot].name} · cần nạp lại`:'Kéo thả hoặc chọn file';$(`${slot}Drop`).classList.toggle('loaded',!!p[slot]);$(`${slot}Drop`).querySelector('.upload-plus').textContent=p[slot]?'✓':'＋';}$('exportBeatName').textContent=p.beat?.name||p.missing_media?.beat?.name||'Chưa nạp beat';$('listenTrack').querySelector('[value=beat]').disabled=!p.beat;$('listenTrack').querySelector('[value=reference]').disabled=!p.reference;$('listenTrack').querySelector('[value=vocals]').disabled=!p.vocals_ready;const s=p.settings;$('aspect').value=s.aspect;$('fontSize').value=s.font_size;$('sungColor').value=s.sung;$('bgColor').value=s.background;$('useBackground').checked=s.use_background;renderAnchors();renderWords();renderSelected();renderStyle();compactProjectView();drawTimeline();}
function renderStyle(){studioStyle();}

function colorAlpha(hex,opacity){return hex+Math.round(Math.max(0,Math.min(100,opacity))/100*255).toString(16).padStart(2,'0');}
const styleFields={titleFont:'title_font',titleEnabled:'title_enabled',titleText:'title_text',titleCredit:'title_credit',titleStart:'title_start',titleEnd:'title_end',titleSize:'title_size',titleWeight:'title_weight',titleItalic:'title_italic',titleX:'title_x',titleY:'title_y',titleColor:'title_color',titleOutline:'title_outline',titleOutlineWidth:'title_outline_width',titleShadow:'title_shadow',titleShadowColor:'title_shadow_color',titleEnter:'title_enter',titleExit:'title_exit',titleEffectSeconds:'title_effect_seconds',logoEnabled:'logo_enabled',logoSize:'logo_size',logoX:'logo_x',logoY:'logo_y',logoOpacity:'logo_opacity',logoEffect:'logo_effect',aspect:'aspect',fontSize:'font_size',fontName:'font',sungColor:'sung',unsungColor:'unsung',bgColor:'background',useBackground:'use_background',outlineSung:'outline_sung',outlineUnsung:'outline_unsung',outlineWidth:'outline_width',outlineSungOpacity:'outline_sung_opacity',outlineUnsungOpacity:'outline_unsung_opacity',lineHeight:'line_height',lineGap:'line_gap',lyricX:'lyric_x',lyricY:'lyric_y',shadowSize:'shadow',shadowColor:'shadow_color',lyricPosition:'position',lineAnimation:'line_animation',italicText:'italic',backgroundType:'background_type',countdownStyle:'countdown',countdownSeconds:'countdown_seconds',roleIcons:'role_icons',roleAName:'role_a_name',roleBName:'role_b_name',roleCName:'role_c_name',roleDName:'role_d_name',roleEName:'role_e_name',roleFName:'role_f_name',roleUnknownName:'role_unknown_name',roleAColor:'role_a_color',roleBColor:'role_b_color',roleCColor:'role_c_color',roleDColor:'role_d_color',roleEColor:'role_e_color',roleFColor:'role_f_color',roleUnknownColor:'role_unknown_color',roleAllColor:'role_all_color',mvStyle:'mv_style',resolution:'resolution',audioNormalize:'audio_normalize',sweepFeather:'sweep_feather',sweepGlow:'sweep_glow',sungGradient:'sung_gradient',gradientTop:'gradient_top',gradientBottom:'gradient_bottom',gradientStart:'gradient_start',gradientEnd:'gradient_end',subtitleBox:'subtitle_box',subtitleBoxColor:'subtitle_box_color',subtitleBoxOpacity:'subtitle_box_opacity',subtitleBoxRadius:'subtitle_box_radius',verticalOffset:'vertical_offset',horizontalOffset:'horizontal_offset',horizontalScale:'horizontal_scale'};
function syncLyricSliders(){
  document.querySelectorAll('[data-lyric-slider]').forEach(slider=>{
    if(document.activeElement===slider)return;
    const input=$(slider.dataset.lyricSlider);
    const value=input.id==='lineGap'&&state.project.settings.line_gap==null
      ?state.layout?.effective_line_gap??0:input.value;
    slider.value=value;
    slider.disabled=!!state.job;
  });
}
function renderStyleControls(){const s=state.project.settings;for(const [id,key] of Object.entries(styleFields)){const input=$(id);if(document.activeElement===input)continue;if(['fontName','titleFont'].includes(id)&&s[key]&&!Array.from(input.options).some(option=>option.value===s[key]))input.add(new Option(s[key]+' · font project',s[key]));if(input.type==='checkbox')input.checked=!!s[key];else input.value=key==='line_gap'&&s[key]==null?'':s[key];}syncLyricSliders();const preset=state.presets.find(p=>Object.entries(p.settings).every(([k,v])=>s[k]===v));$('activePreset').textContent=preset?'Preset: '+preset.name:'Tùy chỉnh của bạn';}
function renderAnchors(){const anchors=state.project.anchors;$('anchorList').innerHTML=anchors.length?anchors.map((a,i)=>`<div class="anchor-item"><span class="anchor-number">${i+1}</span><span>${time(a.ref,true)}</span><i>→</i><span>${time(a.beat,true)}</span><button data-remove-anchor="${i}" aria-label="Xóa mốc ${i+1}">×</button></div>`).join(''):'<p class="hint">Chưa có mốc · tạm dùng cùng thời gian</p>';}
function lineLocked(lineId){return state.project.words.some(word=>word.line===lineId&&word.locked);}
function renderWords(){
  scheduleLayout();$('renderResult').hidden=true;
  const words=state.project.words,estimated=words.filter(word=>word.estimated).length;
  $('estimatedCount').textContent=estimated?`${estimated} từ đang ước lượng`:words.length?'Kéo khối để tinh chỉnh':'Chưa có lời';
  const lines=new Map();words.forEach((word,index)=>{if(!lines.has(word.line))lines.set(word.line,[]);lines.get(word.line).push({word,index});});
  const roleNames={male:state.project.settings.role_a_name||'Nam',female:state.project.settings.role_b_name||'Nữ',all:'Cùng hát',voice_c:state.project.settings.role_c_name,voice_d:state.project.settings.role_d_name,voice_e:state.project.settings.role_e_name,voice_f:state.project.settings.role_f_name,unknown:state.project.settings.role_unknown_name};const roleColors={male:state.project.settings.role_a_color,female:state.project.settings.role_b_color,all:state.project.settings.role_all_color,voice_c:state.project.settings.role_c_color,voice_d:state.project.settings.role_d_color,voice_e:state.project.settings.role_e_color,voice_f:state.project.settings.role_f_color,unknown:state.project.settings.role_unknown_color};
  $('wordList').innerHTML=words.length?[...lines.entries()].map(([lineId,line])=>{
    const role=state.project.settings.singing_mode==='duet'?(line[0].word.role||'all'):'all',locked=line.some(item=>item.word.locked),number=[...lines.keys()].indexOf(lineId)+1;
    return `<div class="word-line${locked?' locked':''}" data-line="${lineId}"><div class="word-line-toolbar"><button type="button" class="word-line-head" data-line-select="${lineId}" aria-label="Chọn câu ${number} · ${esc(line.map(item=>item.word.text).join(' '))}"><span class="line-number">${String(number).padStart(2,'0')}</span><span class="line-time">${time(mapTime(line[0].word.start))}</span><span class="line-role role-${role}" style="border-color:${esc(roleColors[role]||'#a7b3c5')};color:${esc(roleColors[role]||'#a7b3c5')}">${esc(state.project.settings.singing_mode==='solo'?'Đơn ca':roleNames[role]||'Chưa rõ')}</span></button><button type="button" class="line-lock" data-line-lock="${lineId}" aria-pressed="${locked}" title="${locked?'Mở khóa để chỉnh sửa câu':'Khóa câu đã rà soát'}">${locked?'Đã khóa':'Khóa'}</button></div><div class="word-chips">${line.map(({word,index})=>`<button class="word-chip${state.timelineSelection.has(index)?' selected':''}${word.estimated?' estimated':''}${word.locked?' locked':''}" data-word="${index}" title="${time(mapTime(word.start),true)} · ${esc(word.text)}">${esc(word.text)}</button>`).join('')}</div></div>`;
  }).join(''):'<div class="list-empty">Lời và các khối thời gian sẽ hiện ở đây.</div>';
  previewFrame();
}
async function toggleLineLock(lineId){if(state.job)return;const locked=lineLocked(lineId);remember();for(const word of state.project.words)if(word.line===lineId){if(locked)delete word.locked;else word.locked=true;}renderWords();renderSelected();drawTimeline();await saveNow();toast(locked?'Đã mở khóa câu.':'Đã khóa câu đã rà soát.');}
function renderSelected(){renderTimelineEditing();const word=selectedWord(),locked=!!word&&lineLocked(word.line),selected=[...state.timelineSelection].sort((a,b)=>a-b);$('selectedEditor').hidden=!word;$('selectedEmpty').hidden=!!word;$('deleteWordBtn').disabled=!word||locked;const stretch=$('stretchSelection'),stretchButton=$('stretchSelectionBtn');stretch.hidden=!word||selected.length<2;const first=selected.length>1?state.project.words[selected[0]]:null,last=selected.length>1?state.project.words[selected.at(-1)]:null;const range=first&&last?state.project.words.slice(selected[0],selected.at(-1)+1):[];stretchButton.disabled=!range.length||range.some(item=>lineLocked(item.line));if(first&&last){$('stretchStart').value=mapTime(first.start).toFixed(2);$('stretchEnd').value=mapTime(last.end).toFixed(2);}if(word){const mapped=wordOnBeat(word);$('wordText').value=word.text;$('wordStart').value=mapped.start.toFixed(2);$('wordEnd').value=mapped.end.toFixed(2);$('wordRole').value=word.role;$('lineRow').value=word.row==null?'auto':String(word.row);$('wordText').disabled=locked;$('wordStart').disabled=locked;$('wordEnd').disabled=locked;$('wordRole').disabled=locked;$('lineRow').disabled=locked;$('applyWordBtn').disabled=locked;$('splitLineBtn').disabled=locked;$('mergeLineBtn').disabled=locked;$('realignLineBtn').disabled=locked||!state.project.reference;$('toggleSelectedLineLockBtn').textContent=locked?'Mở khóa câu':'Khóa câu';}}
function selectWord(index,seek=true,extend=false){if(extend){if(state.timelineSelection.has(index))state.timelineSelection.delete(index);else state.timelineSelection.add(index);state.selected=state.timelineSelection.has(index)?index:([...state.timelineSelection].at(-1)??-1);}else{state.timelineSelection.clear();state.timelineSelection.add(index);state.selected=index;}document.querySelector('[data-left-tab="lyrics"]').click();showInspector('timing');renderSelected();$('wordList').querySelectorAll('[data-word]').forEach(el=>el.classList.toggle('selected',state.timelineSelection.has(+el.dataset.word)));$('wordList').querySelector(`[data-word="${index}"]`)?.scrollIntoView({block:'nearest'});if(seek&&selectedWord())seekBeat(Math.max(0,mapTime(selectedWord().start)-.5));drawTimeline();}
function selectLine(lineId,seek=true){const indices=state.project.words.map((word,index)=>word.line===lineId?index:-1).filter(index=>index>=0);if(!indices.length)return;state.timelineSelection=new Set(indices);state.selected=indices[0];document.querySelector('[data-left-tab="lyrics"]').click();showInspector('timing');renderSelected();$('wordList').querySelectorAll('[data-word]').forEach(el=>el.classList.toggle('selected',state.timelineSelection.has(+el.dataset.word)));$('wordList').querySelector(`[data-line-select="${lineId}"]`)?.scrollIntoView({block:'nearest'});if(seek)seekBeat(Math.max(0,mapTime(selectedWord().start)-.5));drawTimeline();}
function seekBeat(t){const raw=state.track==='beat'?t:mapTime(t,true);if(Number.isFinite(raw))audio.currentTime=Math.max(0,Math.min(trackDuration()||0,raw));previewFrame();drawTimeline();}
function setAudio(track,position=null,resume=false){
  audio.pause();audio.onloadedmetadata=null;state.track=track;
  if(track!=='beat')state.backingListening=false;
  state.pendingAudio=null;
  $('listenTrack').value=track;renderWaveTrackControls();
  if($('toggleBackingBtn'))renderBackingControls();
  const available=track==='vocals'?state.project.vocals_ready:state.project[track];
  if(!available){audio.removeAttribute('src');audio.load();return;}
  audio.playbackRate=+$('playbackRate').value;
  const raw=position==null?0:(track==='beat'?position:mapTime(position,true));
  state.pendingAudio={position,resume};
  audio.onloadedmetadata=()=>{state.pendingAudio=null;audio.playbackRate=+$('playbackRate').value;audio.currentTime=Math.max(0,Math.min(audio.duration||0,raw));$('seek').max=audio.duration||1;$('totalTime').textContent=time(audio.duration,true);if(resume)audio.play().catch(handleError);previewFrame();};
  const backing=track==='beat'&&state.backingListening&&state.project.backing_status==='ready';
  const slot=backing?'beat_backing':track;
  audio.src=endpoint(`/audio/${slot}`)+`?v=${backing?state.project.backing_vocals.mix:state.project.updated_at}`;
}

async function togglePlay(){if(!audio.getAttribute('src'))return toast('Hãy nạp và chọn track để nghe.');if(audio.paused)await audio.play();else audio.pause();}
function previewFrame(){studioFrame();}

function timelineRange(){const duration=beatDuration();const span=Math.max(3,duration/state.zoom);state.pan=Math.max(0,Math.min(state.pan,Math.max(0,duration-span)));$('pan').max=Math.max(0,duration-span);return {duration,span,start:state.pan};}
function drawTimeline(){studioTimeline();studioWaveforms();}

function eventPoint(event){const rect=$('timeline').getBoundingClientRect(),offset=timelineGutter(rect.width);return {x:event.clientX-rect.left,y:event.clientY-rect.top,width:rect.width-offset,offset};}
$('timeline').addEventListener('pointerdown',event=>{if(state.job||state.manualTiming)return;const p=eventPoint(event),range=timelineRange(),hit=[...state.rectangles].reverse().find(r=>p.x>=r.x&&p.x<=r.x+r.w&&p.y>=r.y&&p.y<=r.y+r.h);if(!hit){if(p.y<24){seekBeat(range.start+Math.max(0,p.x-p.offset)/p.width*range.span);return;}state.marquee={x0:p.x,y0:p.y,x1:p.x,y1:p.y,base:new Set(event.ctrlKey||event.metaKey?state.timelineSelection:[])};$('timeline').setPointerCapture(event.pointerId);return;}const alreadyInMulti=state.timelineSelection.has(hit.i)&&state.timelineSelection.size>1&&!event.ctrlKey&&!event.metaKey;if(alreadyInMulti){state.selected=hit.i;renderWords();renderSelected();drawTimeline();}else selectWord(hit.i,false,event.ctrlKey||event.metaKey);if(event.ctrlKey||event.metaKey)return;if(lineLocked(selectedWord().line)){toast('Câu này đã khóa. Mở khóa để chỉnh mốc.');return;}const word=wordOnBeat(selectedWord());remember();const kind=event.shiftKey||alreadyInMulti?'move':p.x>hit.x+hit.w-6?'end':p.x<hit.x+6?'start':'move';const groupIndices=alreadyInMulti?[...state.timelineSelection].sort((a,b)=>a-b):event.shiftKey?state.project.words.map((w,i)=>w.line===selectedWord().line?i:-1).filter(i=>i>=0):null;state.drag={index:hit.i,startX:p.x,start:word.start,end:word.end,line:word.line,linked:state.project.settings.timeline_linked!==false&&!state.project.words[hit.i-1]?.locked&&!state.project.words[hit.i+1]?.locked,previous:state.project.words[hit.i-1]?wordOnBeat(state.project.words[hit.i-1]):null,next:state.project.words[hit.i+1]?wordOnBeat(state.project.words[hit.i+1]):null,kind,span:range.span,width:p.width,group:groupIndices?groupIndices.filter(i=>!lineLocked(state.project.words[i]?.line)).map(i=>({i,...wordOnBeat(state.project.words[i])})):null};$('timeline').setPointerCapture(event.pointerId);});
$('timeline').addEventListener('pointermove',event=>{const p=eventPoint(event);if(state.marquee){state.marquee.x1=p.x;state.marquee.y1=p.y;const xMin=Math.min(state.marquee.x0,p.x),xMax=Math.max(state.marquee.x0,p.x),yMin=Math.min(state.marquee.y0,p.y),yMax=Math.max(state.marquee.y0,p.y);if(Math.hypot(p.x-state.marquee.x0,p.y-state.marquee.y0)>4){state.timelineSelection=new Set(state.marquee.base);for(const r of state.rectangles){if(r.x+r.w>=xMin&&r.x<=xMax&&r.y+r.h>=yMin&&r.y<=yMax)state.timelineSelection.add(r.i);}const arr=[...state.timelineSelection].sort((a,b)=>a-b);state.selected=arr.length?arr[0]:-1;renderSelected();}drawTimeline();return;}if(!state.drag){const hit=state.rectangles.find(r=>p.x>=r.x&&p.x<=r.x+r.w&&p.y>=r.y&&p.y<=r.y+r.h);$('timeline').style.cursor=hit?(p.x>hit.x+hit.w-6||p.x<hit.x+6?'ew-resize':'grab'):'crosshair';return;}const d=state.drag,delta=(p.x-d.startX)/d.width*d.span,word=state.project.words[d.index];let a=d.start,b=d.end;if(d.kind==='move'){a+=delta;b+=delta;if(a<0){b-=a;a=0;}if(b>beatDuration()){a-=b-beatDuration();b=beatDuration();}}else{const resized=timelineResize(d,d.previous,d.next,d.kind,delta,d.linked,beatDuration());a=resized.start;b=resized.end;d.boundary=resized;}const refA=mapTime(a,true),refB=mapTime(b,true);if(!d.group&&refA>=0&&refB>refA){word.start=refA;word.end=refB;word.estimated=false;if(d.boundary?.previousEnd!=null){const neighbor=state.project.words[d.index-1];neighbor.end=mapTime(d.boundary.previousEnd,true);neighbor.estimated=false;}if(d.boundary?.nextStart!=null){const neighbor=state.project.words[d.index+1];neighbor.start=mapTime(d.boundary.nextStart,true);neighbor.estimated=false;}}if(d.group&&d.group.length){const shift=Math.max(-Math.min(...d.group.map(w=>w.start)),Math.min(beatDuration()-Math.max(...d.group.map(w=>w.end)),delta));for(const w of d.group){const item=state.project.words[w.i],ra=mapTime(w.start+shift,true),rb=mapTime(w.end+shift,true);if(ra>=0&&rb>ra){item.start=ra;item.end=rb;item.estimated=false;}}}renderSelected();drawTimeline();previewFrame();});
function finishDrag(event){if(state.marquee){const m=state.marquee;state.marquee=null;if(Math.hypot(m.x1-m.x0,m.y1-m.y0)<=4){const pt=eventPoint(event),range=timelineRange();seekBeat(range.start+Math.max(0,m.x0-pt.offset)/pt.width*range.span);}else{renderWords();renderSelected();}drawTimeline();return;}if(!state.drag)return;state.drag=null;renderWords();dirty();}
$('timeline').addEventListener('pointerup',finishDrag);$('timeline').addEventListener('pointercancel',finishDrag);


function renderAlignmentPreview(candidate){
  const lines=candidate.line_ids||[],oldWords=candidate.old_words||[],newWords=candidate.new_words||[];
  $('alignPreviewContent').innerHTML=lines.map((lineId,index)=>{
    const old=oldWords.filter(word=>word.line===lineId),next=newWords.filter(word=>word.line===lineId),range=items=>items.length?`${time(mapTime(Math.min(...items.map(w=>w.start))),true)} – ${time(mapTime(Math.max(...items.map(w=>w.end))),true)}`:'Chưa có mốc';
    return `<article class="align-preview-row"><h3>Câu ${index+1}</h3><div class="align-preview-side"><strong>Hiện tại · ${range(old)}</strong><p>${esc(old.map(word=>word.text).join(' '))}</p></div><div class="align-preview-side"><strong>Đề xuất · ${range(next)}</strong><p>${esc(next.map(word=>word.text).join(' '))}</p><small>${next.length} từ · ${next.filter(word=>word.estimated).length} từ cần rà</small></div></article>`;
  }).join('');
  $('applyAlignmentBtn').disabled=!newWords.length;
  if(!$('alignPreviewDialog').open)$('alignPreviewDialog').showModal();
}
function renderRolePreview(result){
  state.pendingRoles=result;const groups=new Map();
  for(const word of state.project.words){if(!groups.has(word.line))groups.set(word.line,[]);groups.get(word.line).push(word);}
  const settings=state.project.settings||{},roleNames={all:'Cùng hát',male:settings.role_a_name||'Nam',female:settings.role_b_name||'Nữ',voice_c:settings.role_c_name||'Giọng 3',voice_d:settings.role_d_name||'Giọng 4',voice_e:settings.role_e_name||'Giọng 5',voice_f:settings.role_f_name||'Giọng 6',unknown:settings.role_unknown_name||'Chưa rõ'};
  const choices=[];for(let index=0;index<(result.lines||[]).length;index++){const line=result.lines[index],role=result.roles?.[index],words=groups.get(line)||[];if(!role||!words.length||!['all','unknown'].includes(words[0].role||'all'))continue;choices.push({line,role,words,confidence:result.confidences?.[index]||'register'});}
  const explanation=roleAnalysisMessage(result);
  $('roleSuggestionSummary').innerHTML=`<p>${esc(explanation)}</p><p class="hint">Có thể áp dụng ${choices.length} câu · giữ nguyên ${result.uncertain||0} câu chưa đủ rõ.</p>`;
  const options=[['','Giữ nguyên'],...Object.entries(roleNames).map(([role,name])=>[role,name])];
  $('roleSuggestionList').innerHTML=choices.map(item=>{const index=result.lines.indexOf(item.line),confidence={high:'Khớp rõ',medium:'Khớp vừa',register:'Theo dải cao độ'}[item.confidence]||'Gợi ý';return `<label class="role-suggestion-row"><span><strong>Câu ${index+1} · ${time(mapTime(item.words[0].start),true)}</strong><small>${esc(item.words.map(word=>word.text).join(' '))} · ${confidence}</small></span><select data-role-choice="${item.line}" aria-label="Vai gợi ý cho câu ${index+1}">${options.map(([role,name])=>`<option value="${role}"${role===item.role?' selected':''}>${esc(name)}</option>`).join('')}</select></label>`;}).join('')||'<p class="hint">Không có câu nào đủ rõ để gợi ý. Bạn có thể gán vai thủ công.</p>';
  $('applyRoleSuggestionsBtn').disabled=!choices.length;$('rolePreviewDialog').showModal();
}
function discardAlignment(){state.pendingAlignment=null;if($('alignPreviewDialog').open)$('alignPreviewDialog').close();}
async function applyAlignment(){
  const candidate=state.pendingAlignment;if(!candidate?.words?.length)throw Error('Không còn đề xuất căn để áp dụng.');
  remember(state.project.text||textFromWords());state.project.words=candidate.words;state.project.text=candidate.text||textFromWords();$('lyricsText').value=state.project.text;state.selected=-1;state.timelineSelection.clear();
  renderWords();renderSelected();drawTimeline();discardAlignment();await saveNow();toast(`Đã áp dụng căn lại ${candidate.new_words.length} từ. Các câu khác được giữ nguyên.`);
}

function friendlyJobMessage(job){
  const phase=String(job.message||'');
  if(['separate','backing','tool-backing'].includes(job.kind)){
    if(job.state==='done')return 'Tách nhạc hoàn tất.';
    if(job.state==='cancelled')return 'Đã hủy tách nhạc.';
    if(job.state==='error')return 'Không tách được nhạc. Hãy thử lại.';
    if(job.state==='queued')return 'Đang chuẩn bị tách nhạc…';
    if(job.kind!=='separate'||/giọng bè|phân tách.*bè|căn.*bè|trộn.*bè/i.test(phase)||Number(job.progress)>42)return 'Đang xử lý giọng bè…';
    if(/công cụ|thiết bị|model|mô hình/i.test(phase))return 'Đang chuẩn bị tách nhạc…';
    if(/YouTube/i.test(phase))return 'Đang tải nhạc từ YouTube…';
    return 'Đang tách nhạc…';
  }
  if(job.kind==='align-region')return 'Đang căn lại câu đã chọn…';
  if(job.kind==='sync-suggest')return 'Đang dò mốc giữa bản hát và beat…';
  if(job.kind==='analyze')return phase.includes('căn lời có sẵn')?'Đang căn lời theo nhạc…':'Đang nhận lời và căn thời gian…';
  if(job.kind==='roles')return 'Đang phân tích vocal để gợi ý vai…';
  if(job.kind==='render')return 'Đang tạo video karaoke…';
  if(job.kind==='upload')return ({reference:'Đang nạp bản có vocal…',beat:'Đang nạp beat…',background:'Đang nạp nền…',logo:'Đang nạp logo…',countdown:'Đang nạp ảnh đếm ngược…'})[job.uploadSlot]||'Đang nạp file…';
  if(job.kind==='model')return 'Đang tải model…';
  if(job.kind==='estimate')return 'Đang căn thời gian lời…';
  return 'Đang xử lý…';
}
function busy(job,finishedKind=null){if(job?.kind==='render')state.renderJob=job;else if(job)state.job=job;else if(finishedKind==='render')state.renderJob=null;else if(finishedKind)state.job=null;else{state.job=null;state.renderJob=null;}const blocking=!!state.job,visible=state.job||state.renderJob;renderBackingControls();$('timelineLinked').disabled=blocking;document.querySelectorAll('.style-panel input,.style-panel select,.style-panel button,.source-panel select').forEach(el=>el.disabled=blocking);document.body.classList.toggle('is-busy',blocking);$('jobBar').hidden=!visible;for(const id of ['referenceInput','beatInput','backgroundInput','videoSeparateBtn','videoReferenceBtn','newBtn','projectsBtn','exportBtn','aiModelBtn','projectName','lyricsText','analyzeBtn','alignLyricsBtn','timelineAutoBtn','estimateBtn','renderBtn','previewRenderBtn','splitLineBtn','mergeLineBtn','alternateRolesBtn','autoRolesBtn','swapRolesBtn','applyWordBtn','addAnchorBtn','realignLineBtn','toggleSelectedLineLockBtn','applyLyricsEditsBtn','deleteWordBtn'])$(id).disabled=blocking;for(const id of ['newBtn','projectsBtn','renderBtn','previewRenderBtn'])$(id).disabled=blocking||!!state.renderJob;if(visible){$('jobTitle').textContent=friendlyJobMessage(visible);$('jobPercent').textContent=Math.round(visible.progress||0)+'%';$('jobProgress').style.width=(visible.progress||0)+'%';}}
function syncAudition(slot, label, seconds){
  if(!Number.isFinite(seconds))return '';
  const start=Math.max(0,seconds-2).toFixed(2);
  const end=(seconds+3).toFixed(2);
  return `<label>${label}<audio controls preload="none" src="${endpoint('/audio/'+slot)}#t=${start},${end}"></audio></label>`;
}
$('syncPreviewDialog').addEventListener('play',event=>{
  if(!(event.target instanceof HTMLAudioElement))return;
  audio.pause();
  for(const player of $('syncPreviewDialog').querySelectorAll('audio')){
    if(player!==event.target)player.pause();
  }
},true);
$('syncPreviewDialog').addEventListener('close',()=>{
  $('syncPreviewDialog').querySelectorAll('audio').forEach(player=>player.pause());
});
function renderSyncPreview(candidate){
  state.pendingSync=candidate;
  const current=state.project.anchors||[],regions=candidate.regions||[];
  const labels={matched:'Khớp',shifted:'Lệch',structure:'Có thể đổi cấu trúc',added:'Có thể được thêm',cut:'Có thể bị cắt',repeated:'Có thể lặp',unmatched:'Không tìm thấy',uncertain:'Cần nghe kiểm tra'};
  const structuralCount=regions.filter(region=>['structure','added','cut'].includes(region.status)).length;
  const repeatedCount=regions.filter(region=>region.status==='repeated').length;
  const unmatchedCount=regions.filter(region=>region.status==='unmatched').length;
  const uncertainCount=regions.filter(region=>region.status==='uncertain').length;
  $('syncCandidateSummary').innerHTML=`<strong>${esc(candidate.structure)}</strong><p class="hint">Độ tin cậy: ${esc(candidate.confidence)} · tương đồng ${Math.round(candidate.score*100)}% · vùng khớp ${Math.round(candidate.coverage*100)}%</p><p class="hint">Tốc độ beat/bản hát: ${Number(candidate.beat_per_reference).toFixed(3)}× · lệch đầu: ${Number(candidate.offset_seconds).toFixed(2)} giây</p><p class="hint">Đề xuất chỉ thay khi bạn bấm Áp dụng. ${candidate.confidence.startsWith('Thấp')?'Nên nghe hai bản và đặt mốc thủ công nếu đề xuất chưa đúng.':'Hãy nghe thử trước khi tiếp tục.'}</p>${regions.length?`<p class="hint">Đã rà ${regions.length} vùng · ${structuralCount} vùng có thể bị thêm/cắt · ${repeatedCount} vùng có thể lặp · ${unmatchedCount} vùng không tìm thấy · ${uncertainCount} vùng chưa rõ.</p>`:''}`;
  $('syncRegionList').innerHTML=regions.length?`<details class="sync-regions"><summary>Chi tiết so khớp từng vùng</summary><div class="sync-region-items">${regions.map(region=>{
    const start=time(region.reference_start,true),end=time(region.reference_end,true);
    const beat=region.beat_start==null?'chưa rõ':`${time(region.beat_start,true)}–${time(region.beat_end,true)}`;
    const alternate=region.alternate_beat_start==null?'':` · vùng tương tự khác ở ${time(region.alternate_beat_start,true)}`;
    const status=labels[region.status]||'Cần kiểm tra';
    return `<article class="sync-region ${esc(region.status)}"><div><strong>${esc(status)}</strong><span>Bản hát ${start}–${end} · Beat ${beat}${alternate}</span></div><small>${esc(region.note||'')} · tương đồng ${Math.round((region.similarity||0)*100)}%${region.offset_seconds==null?'':` · lệch ${Number(region.offset_seconds).toFixed(1)} giây`}</small><div class="sync-audition">${syncAudition('reference','Bản hát',region.reference_start)}${syncAudition('beat','Beat đề xuất',region.beat_start)}${syncAudition('beat','Beat ở vị trí lặp',region.alternate_beat_start)}</div></article>`
  }).join('')}</div><p class="hint">Các dấu hiệu này là gợi ý để nghe kiểm tra; không tự thêm hoặc sửa mốc.</p></details>`:'';
  $('syncAnchorComparison').innerHTML=`<div class="sync-anchor-columns"><div><strong>Mốc hiện tại</strong>${current.length?current.map((anchor,index)=>`<p>${index+1}. ${time(anchor.ref,true)} → ${time(anchor.beat,true)}</p>`).join(''):'<p class="hint">Chưa có</p>'}</div><div><strong>Mốc đề xuất</strong>${candidate.anchors.map((anchor,index)=>`
    <div>
      <p>${index+1}. ${time(anchor.ref,true)} → ${time(anchor.beat,true)}</p>
      ${syncAudition('reference','Bản hát',anchor.ref)}
      ${syncAudition('beat','Beat',anchor.beat)}
    </div>`).join('')}</div></div>`;
  $('syncPreviewDialog').showModal();
}
function renderBatchSummary(result){const items=result?.items||[];$('batchResultSummary').innerHTML=`<p class="hint">Hoàn tất ${Number(result?.completed)||0}/${items.length} video · lỗi ${Number(result?.failed)||0}. Mỗi video dùng snapshot riêng đã chụp khi vào hàng đợi.</p>${result?.failed?`<button class="secondary" data-retry-batch="${esc(result.batch_id)}">Thử dựng lại video lỗi</button>`:''}`;$('batchResultItems').innerHTML=items.map(item=>`<div class="version-item"><div><strong>${esc(item.name)}</strong><small>${item.status==='done'?'Đã dựng xong':esc(item.error||'Chưa hoàn tất')}</small></div>${item.result?.video?`<div class="batch-downloads">${item.result.qa_contact?`<a class="ghost" href="${esc(item.result.qa_contact)}" target="_blank" rel="noopener">Ảnh mẫu</a>`:''}<a class="secondary batch-download" href="${esc(item.result.video)}" download>Tải video</a></div>`:''}</div>`).join('')||'<p class="hint">Hàng đợi chưa có project.</p>';$('batchResultDialog').showModal();}
async function followJob(job,context={}){job={...job,...context};busy(job);const pid=job.project_id;try{while(true){await new Promise(resolve=>setTimeout(resolve,700));job={...await api(`/api/jobs/${job.id}`),...context};busy(job);if(['done','error','cancelled'].includes(job.state))break;}busy(null,job.kind);if(job.state==='error')throw Error(job.message);if(job.state==='cancelled'){toast('Đã hủy tác vụ.');return;}if(job.kind==='align-region'){state.pendingAlignment=job.result;renderAlignmentPreview(job.result);toast(job.result?.cache_hit?'Đã dùng kết quả căn lời đã lưu. Xem lại từng câu rồi chọn áp dụng hoặc bỏ đề xuất.':'Đã căn xong. Xem lại từng câu rồi chọn áp dụng hoặc bỏ đề xuất.');return job;}if(job.kind==='sync-suggest'){renderSyncPreview(job.result);return job;}if(job.kind==='batch-render'){renderBatchSummary(job.result);return job;}if(job.kind==='render')await saveNow();state.project=await api(`/api/projects/${pid}`);state.layout=null;state.selected=-1;state.timelineSelection.clear();renderAll();if(job.kind==='analyze'&&state.track==='vocals')setAudio('vocals',beatNow());if(job.kind==='upload'){
  if(job.result?.previous_version_id){
    state.undo.push({versionId:job.result.previous_version_id});
    if(state.undo.length>60)state.undo.shift();
    state.redo=[];
    updateHistoryButtons();
  }
  setAudio(state.project.beat?'beat':'reference',0);if(job.result?.relinked){const slot=job.result.slot;const name={reference:'bản hát',beat:'beat',background:'nền',logo:'logo',countdown:'ảnh đếm ngược'}[slot]||'media';toast(['reference','beat'].includes(slot)?'Đã nhận đúng file nguồn theo dấu vân tay; giữ nguyên lời và mốc đồng bộ.':'Đã khôi phục '+name+' đúng dấu vân tay.');}}if(job.kind==='analyze')toast(`Đã căn ${job.result.words} từ${job.result.cache_hit?' · dùng kết quả đã lưu':''}. Nghe và chỉnh lại những chỗ AI lệch.`);if(job.kind==='roles')renderRolePreview(job.result);if(job.kind==='render'){const url=job.result.video;$('downloadVideo').textContent='Tải video '+(job.result.format||'mp4').toUpperCase()+' ↓';$('exportSavedPath').textContent=job.result.output_path?'Đã lưu: '+job.result.output_path:'';$('downloadVideo').href=url;$('renderPreview').src=url;$('renderQaContact').hidden=!job.result.qa_contact;if(job.result.qa_contact)$('renderQaContact').src=job.result.qa_contact;$('renderResult').hidden=false;$('exportDialog').showModal();toast('Đã dựng video từ Beat của bạn.');}return job;}catch(error){busy(null,job.kind);handleError(error);return null;}}
async function upload(slot,file){if(!file||state.job||state.manualTiming)return;if(slot==='reference'&&state.project.reference&&state.project.words.length&&!confirm('Đổi bản hát tham chiếu sẽ xóa các mốc từ đã nhận dạng. Tiếp tục?'))return;await saveNow();const body=new FormData();body.append('file',file);busy({kind:'upload',uploadSlot:slot,message:'Đang nạp file…',progress:0});try{const job=await api(endpoint(`/media/${slot}`),{method:'POST',body});followJob(job,{uploadSlot:slot});}catch(error){busy(null,'upload');throw error;}}
for(const slot of ['reference','beat']){const input=$(`${slot}Input`),drop=$(`${slot}Drop`);input.onchange=wrap(async()=>{await upload(slot,input.files[0]);input.value='';});drop.addEventListener('dragover',e=>{e.preventDefault();drop.classList.add('dragging');});drop.addEventListener('dragleave',()=>drop.classList.remove('dragging'));drop.addEventListener('drop',wrap(async e=>{e.preventDefault();drop.classList.remove('dragging');await upload(slot,e.dataTransfer.files[0]);}));drop.addEventListener('keydown',e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();e.stopPropagation();input.click();}});}
$('logoInput').onchange=wrap(async()=>{await upload('logo',$('logoInput').files[0]);$('logoInput').value='';});
$('backgroundInput').onchange=wrap(async()=>{await upload('background',$('backgroundInput').files[0]);$('backgroundInput').value='';});
$('analyzeBtn').onclick=wrap(async()=>{if(state.project.words.some(word=>word.locked))throw Error('Mở khóa các câu đã khóa trước khi nhận lời và căn lại toàn bài.');const lyrics=$('lyricsText').value.trim();await saveNow();if(state.project.words.length&&!confirm('Nhận dạng/căn lại sẽ thay các mốc từ hiện có. Tiếp tục?'))return;followJob(await sendAnalyze(lyrics));});
$('alignLyricsBtn').onclick=wrap(async()=>{if(state.project.words.some(word=>word.locked))throw Error('Mở khóa các câu đã khóa trước khi căn lại toàn bài.');const lyrics=$('lyricsText').value.trim();if(!lyrics)throw Error('Hãy dán lời đúng vào ô lời trước.');await saveNow();if(state.project.words.length&&!confirm('Căn lại lời sẽ thay các mốc từ hiện có. Tiếp tục?'))return;followJob(await sendAnalyze(lyrics));});
$('captureRef').onclick=()=>{if(state.track==='beat')return toast('Chuyển sang nghe bản có giọng hoặc vocal, tìm vị trí rồi bấm lấy mốc.');$('anchorRef').value=audio.currentTime.toFixed(2);toast('Đã ghi vị trí trên bản hát.');};
$('captureBeat').onclick=()=>{if(state.track!=='beat')return toast('Chuyển sang nghe beat gốc, tìm nhịp tương ứng rồi bấm lấy mốc.');$('anchorBeat').value=audio.currentTime.toFixed(2);toast('Đã ghi vị trí trên beat gốc.');};
$('addAnchorBtn').onclick=wrap(async()=>{const ref=+$('anchorRef').value,beat=+$('anchorBeat').value;if(!Number.isFinite(ref)||!Number.isFinite(beat)||ref<0||beat<0)throw Error('Hai mốc phải là số không âm.');const anchors=[...state.project.anchors,{ref,beat}].sort((a,b)=>a.ref-b.ref);for(let i=1;i<anchors.length;i++)if(anchors[i].ref-anchors[i-1].ref<.02||anchors[i].beat-anchors[i-1].beat<.02)throw Error('Các mốc phải cùng tăng ở cả hai bản; không được trùng hoặc đảo thứ tự.');remember();state.project.anchors=anchors;await saveNow();renderAnchors();renderWords();renderSelected();drawTimeline();toast('Đã thêm mốc, lời được chuyển sang thời gian beat.');});
$('applySyncSuggestion').onclick=wrap(async()=>{const candidate=state.pendingSync;if(!candidate)return;remember();state.project=await api(endpoint(),{method:'PATCH',body:{anchors:candidate.anchors}});state.pendingSync=null;$('syncPreviewDialog').close();renderAnchors();renderWords();renderSelected();drawTimeline();updateHistoryButtons();toast('Đã áp dụng mốc đồng bộ. Hãy nghe thử các điểm đầu và cuối.');});
$('discardSyncSuggestion').onclick=()=>{state.pendingSync=null;$('syncPreviewDialog').close();};$('syncPreviewDialog').addEventListener('close',()=>{state.pendingSync=null;});
$('anchorList').onclick=wrap(async e=>{const button=e.target.closest('[data-remove-anchor]');if(!button||state.job||state.manualTiming)return;remember();state.project.anchors.splice(+button.dataset.removeAnchor,1);await saveNow();renderAnchors();renderWords();renderSelected();drawTimeline();});
$('wordList').onclick=wrap(async e=>{const lock=e.target.closest('[data-line-lock]');if(lock){e.preventDefault();e.stopPropagation();await toggleLineLock(+lock.dataset.lineLock);return;}const line=e.target.closest('[data-line-select]');if(line){selectLine(+line.dataset.lineSelect);return;}const button=e.target.closest('[data-word]');if(button)selectWord(+button.dataset.word,true,e.ctrlKey||e.metaKey);});
$('applyRoleSuggestionsBtn').onclick=wrap(async()=>{const result=state.pendingRoles;if(!result)return;const choices=new Map([...$('roleSuggestionList').querySelectorAll('[data-role-choice]')].map(select=>[+select.dataset.roleChoice,select.value]));remember();let changed=0;for(const [line,role] of choices){if(!role)continue;const words=state.project.words.filter(word=>word.line===line);if(!words.length||!['all','unknown'].includes(words[0].role||'all'))continue;for(const word of words)word.role=role;changed++;}state.project.settings.singing_mode='duet';renderWords();renderSelected();renderStyle();drawTimeline();await saveNow();state.pendingRoles=null;$('rolePreviewDialog').close();toast(`Đã áp dụng gợi ý cho ${changed} câu. Các câu chưa rõ được giữ nguyên.`);});
$('discardRoleSuggestionsBtn').onclick=()=>{state.pendingRoles=null;$('rolePreviewDialog').close();};$('rolePreviewDialog').addEventListener('close',()=>{state.pendingRoles=null;});
$('toggleSelectedLineLockBtn').onclick=wrap(async()=>{const word=selectedWord();if(word)await toggleLineLock(word.line);});
$('realignLineBtn').onclick=wrap(async()=>{const word=selectedWord();if(!word)return;if(lineLocked(word.line))throw Error('Mở khóa câu trước khi căn lại.');await saveNow();followJob(await api(endpoint('/align-region'),{method:'POST',body:{line_ids:[word.line],model:selectedAiModel(),language:selectedAiLanguage(),device:'auto'}}));});
$('applyLyricsEditsBtn').onclick=wrap(applyLyricsEditsPreservingTiming);
$('discardAlignmentBtn').onclick=discardAlignment;$('applyAlignmentBtn').onclick=wrap(applyAlignment);$('alignPreviewDialog').addEventListener('close',()=>{state.pendingAlignment=null;});
$('applyWordBtn').onclick=wrap(async()=>{const word=selectedWord();if(!word)return;if(lineLocked(word.line))throw Error('Mở khóa câu trước khi chỉnh thời gian.');const a=+$('wordStart').value,b=+$('wordEnd').value,start=mapTime(a,true),end=mapTime(b,true),text=$('wordText').value.trim();if(!text||!Number.isFinite(a+b)||a<0||b<=a||start<0||end<=start)throw Error('Kiểm tra nội dung và thời gian. Mốc kết thúc phải sau bắt đầu; nếu ra ngoài bản hát hãy sửa mốc đồng bộ.');remember();word.text=text;word.start=start;word.end=end;word.estimated=false;for(const item of state.project.words)if(item.line===word.line){item.role=$('wordRole').value;if($('lineRow').value==='auto')delete item.row;else item.row=+$('lineRow').value;}state.project.text=textFromWords();$('lyricsText').value=state.project.text;renderWords();drawTimeline();await saveNow();toast('Đã sửa từ và thời gian theo beat.');});
$('deleteWordBtn').onclick=()=>{if(!selectedWord()||lineLocked(selectedWord().line))return;remember();state.project.words.splice(state.selected,1);state.project.text=textFromWords();$('lyricsText').value=state.project.text;state.selected=-1;state.timelineSelection.clear();renderWords();renderSelected();drawTimeline();dirty();};
async function moveHistory(from,to){
  if(state.job||!from.length)return;
  const entry=from.at(-1);
  if(!entry.versionId){
    to.push(historySnapshot());
    restoreHistorySnapshot(from.pop());
    return;
  }
  clearTimeout(state.saveTimer);
  const saved=saveNow();
  busy({kind:'history',progress:0});
  try{
    await saved;
    const {previous_version_id,...project}=await api(endpoint('/versions/'+entry.versionId+'/restore'),{method:'POST',body:{}});
    from.pop();
    to.push({versionId:previous_version_id});
    state.project=project;
    state.layout=null;
    state.selected=-1;
    state.timelineSelection.clear();
    renderAll();
    setAudio(project.beat?'beat':'reference',0);
  }finally{
    busy(null,'history');
    updateHistoryButtons();
  }
}
const undo=wrap(()=>moveHistory(state.undo,state.redo));
const redo=wrap(()=>moveHistory(state.redo,state.undo));
$('undoBtn').onclick=undo;
$('redoBtn').onclick=redo;
$('stretchSelectionBtn').onclick=wrap(async()=>{
  if(state.job)return;
  const selected=[...state.timelineSelection].sort((a,b)=>a-b);
  if(selected.length<2)throw Error('Chọn ít nhất hai từ làm mốc đầu và cuối.');
  const first=selected[0],last=selected.at(-1),range=state.project.words.slice(first,last+1);
  if(range.some(word=>lineLocked(word.line)))throw Error('Mở khóa các câu trong vùng chọn trước khi co giãn.');
  const sourceStart=mapTime(range[0].start),sourceEnd=mapTime(range.at(-1).end);
  const targetStart=+$('stretchStart').value,targetEnd=+$('stretchEnd').value;
  if(!Number.isFinite(sourceStart+sourceEnd+targetStart+targetEnd)||sourceEnd<=sourceStart||targetStart<0||targetEnd<=targetStart||targetEnd>beatDuration()+.05)throw Error('Kiểm tra hai mốc beat; mốc cuối phải sau mốc đầu và nằm trong thời lượng beat.');
  const scale=(targetEnd-targetStart)/(sourceEnd-sourceStart);
  remember();
  for(const word of range){const start=mapTime(word.start),end=mapTime(word.end);word.start=mapTime(targetStart+(start-sourceStart)*scale,true);word.end=mapTime(targetStart+(end-sourceStart)*scale,true);}
  renderWords();renderSelected();drawTimeline();await saveNow();toast(`Đã co giãn ${range.length} từ theo khoảng beat mới.`);
});

function lyricTokenKey(value){return String(value).normalize('NFC').toLocaleLowerCase().replace(/[^\p{L}\p{N}]/gu,'');}
function lyricMatches(oldWords,newTokens){
  const a=oldWords.map(word=>lyricTokenKey(word.text)),b=newTokens.map(item=>lyricTokenKey(item.text)),n=a.length,m=b.length;
  if(n*m<=2000000){
    const width=m+1,dp=new Uint32Array((n+1)*width);
    for(let i=n-1;i>=0;i--)for(let j=m-1;j>=0;j--){const at=i*width+j;dp[at]=a[i]===b[j]?1+dp[(i+1)*width+j+1]:Math.max(dp[(i+1)*width+j],dp[at+1]);}
    const pairs=[];let i=0,j=0;while(i<n&&j<m){if(a[i]===b[j]){pairs.push([i,j]);i++;j++;}else if(dp[(i+1)*width+j]>=dp[i*width+j+1])i++;else j++;}return pairs;
  }
  const positions=new Map();a.forEach((key,index)=>{if(!positions.has(key))positions.set(key,[]);positions.get(key).push(index);});
  const pairs=[];let cursor=0;for(let j=0;j<m;j++){const list=positions.get(b[j]);if(!list)continue;let lo=0,hi=list.length;while(lo<hi){const mid=(lo+hi)>>1;if(list[mid]<cursor)lo=mid+1;else hi=mid;}if(lo<list.length){const i=list[lo];pairs.push([i,j]);cursor=i+1;}}return pairs;
}
function applyLyricsEditsPreservingTiming(){
  if(state.job||state.manualTiming)return;
  const raw=$('lyricsText').value,lines=raw.split(/\r?\n/).map(line=>line.trim().split(/\s+/).filter(Boolean)).filter(line=>line.length),newTokens=[];
  lines.forEach((tokens,line)=>tokens.forEach(text=>newTokens.push({text,line})));
  const oldWords=state.project.words,oldLines=new Map();for(const word of oldWords){if(!oldLines.has(word.line))oldLines.set(word.line,[]);oldLines.get(word.line).push(word);}
  const newLineKeys=lines.map(tokens=>tokens.map(lyricTokenKey)),lineStarts=[];let lineOffset=0;
  lines.forEach((tokens,line)=>{lineStarts[line]=lineOffset;lineOffset+=tokens.length;});
  const oldIndexByWord=new Map(oldWords.map((word,index)=>[word,index])),lockedGroups=[...oldLines.values()].filter(words=>words.some(word=>word.locked)),anchors=[];let targetLineCursor=0;
  for(const words of lockedGroups){
    const keys=words.map(word=>lyricTokenKey(word.text));let found=-1;
    for(let line=targetLineCursor;line<newLineKeys.length;line++)if(newLineKeys[line].length===keys.length&&newLineKeys[line].every((key,i)=>key===keys[i])){found=line;break;}
    if(found<0)throw Error('Có câu đang khóa. Mở khóa hoặc giữ nguyên nội dung và thứ tự các câu đã khóa trước khi áp dụng sửa lời.');
    words.forEach((word,i)=>anchors.push([oldIndexByWord.get(word),lineStarts[found]+i]));targetLineCursor=found+1;
  }
  const pairs=[];let oldCursor=0,newCursor=0;
  for(const [oldAnchor,newAnchor] of anchors){for(const [oi,ni] of lyricMatches(oldWords.slice(oldCursor,oldAnchor),newTokens.slice(newCursor,newAnchor)))pairs.push([oldCursor+oi,newCursor+ni]);pairs.push([oldAnchor,newAnchor]);oldCursor=oldAnchor+1;newCursor=newAnchor+1;}
  for(const [oi,ni] of lyricMatches(oldWords.slice(oldCursor),newTokens.slice(newCursor)))pairs.push([oldCursor+oi,newCursor+ni]);
  const result=Array(newTokens.length);
  for(const [oldIndex,newIndex] of pairs)result[newIndex]={...oldWords[oldIndex],text:newTokens[newIndex].text,line:newTokens[newIndex].line};
  const boundaries=[[-1,-1],...pairs,[oldWords.length,newTokens.length]];
  for(let block=0;block<boundaries.length-1;block++){
    const [prevOld,prevNew]=boundaries[block],[nextOld,nextNew]=boundaries[block+1],oldBlock=oldWords.slice(prevOld+1,nextOld),newStart=prevNew+1,newEnd=nextNew,newCount=newEnd-newStart;
    if(!newCount)continue;
    const spanStart=oldBlock.length?oldBlock[0].start:(prevOld>=0?oldWords[prevOld].end:null),spanEnd=oldBlock.length?oldBlock[oldBlock.length-1].end:(nextOld<oldWords.length?oldWords[nextOld].start:null);
    if(oldBlock.length){
      const a=oldBlock[0].start,b=Math.max(a+.08,oldBlock[oldBlock.length-1].end),weights=newTokens.slice(newStart,newEnd).map(item=>Math.max(1,[...item.text].length)),total=weights.reduce((sum,value)=>sum+value,0);
      for(let k=0,elapsed=0;k<newCount;k++){
        const item=newTokens[newStart+k],template=oldBlock[Math.min(oldBlock.length-1,Math.floor(k*oldBlock.length/newCount))],start=a+(b-a)*elapsed/total;elapsed+=weights[k];const end=a+(b-a)*elapsed/total;
        const word={...template,text:item.text,line:item.line,start,end,estimated:true};delete word.locked;result[newStart+k]=word;
      }
    }else{
      let left=spanStart,right=spanEnd;if(left==null&&right!=null)left=Math.max(0,right-newCount*.32);if(left==null)left=0;
      if(right==null)right=Math.min(state.project.reference?.duration||state.project.beat?.duration||left+newCount*.32,left+newCount*.32);
      if(right-left<newCount*.1)right=left+newCount*.32;const step=(right-left)/newCount;
      for(let k=0;k<newCount;k++){
        const item=newTokens[newStart+k],neighbor=prevOld>=0?oldWords[prevOld]:nextOld<oldWords.length?oldWords[nextOld]:null,start=Math.max(0,left+step*k+Math.min(.02,step*.08)),end=Math.max(start+.06,Math.min(right,left+step*(k+1)-Math.min(.02,step*.08)));
        const word={text:item.text,start,end,line:item.line,role:neighbor?.role||'all',estimated:true};if(Number.isInteger(neighbor?.row)&&neighbor.row>=0&&neighbor.row<6)word.row=neighbor.row;result[newStart+k]=word;
      }
    }
  }
  if(!newTokens.length&&oldWords.length&&!confirm('Xóa toàn bộ lời và mốc từ khỏi project?'))return;
  remember(state.project.text||oldWords.map(word=>word.text).join(' '));
  state.project.words=result.filter(Boolean);state.project.text=raw;state.selected=-1;state.timelineSelection.clear();renderWords();renderSelected();drawTimeline();dirty();
  const estimated=state.project.words.filter(word=>word.estimated).length;
  toast(`Đã áp dụng lời. Giữ mốc cho ${pairs.length} từ khớp; ${estimated} từ cần nghe và rà lại.`);
}

const projectNameInput=$('projectName');projectNameInput.addEventListener('keydown',event=>{if(['INPUT','TEXTAREA','SELECT'].includes(event.target.tagName)&&!projectNameInput.dataset.historyActive&&!['Tab','Shift','Control','Alt','Meta'].includes(event.key)){remember();projectNameInput.dataset.historyActive='1';}});projectNameInput.onblur=()=>delete projectNameInput.dataset.historyActive;projectNameInput.oninput=()=>{state.project.name=projectNameInput.value;$('previewTitle').textContent=state.project.name.toUpperCase();dirty();};$('lyricsText').oninput=()=>dirty();
for(const [id,key] of Object.entries(styleFields)){const input=$(id);const update=event=>{if(state.job)return;const numeric=['number','range'].includes(input.type);const continuous=input.type==='range'||input.hasAttribute('data-lyric-number');if(continuous){if(event.type==='input'&&!input.dataset.historyActive){remember();input.dataset.historyActive='1';}}else if(event.type==='change')remember();let value=input.type==='checkbox'?input.checked:numeric?Number(input.value):input.value;const emptyGap=key==='line_gap'&&input.value.trim()==='';if(numeric&&input.value.trim()===''&&!emptyGap)return;if(emptyGap)value=null;if(numeric&&!emptyGap){if(!Number.isFinite(value))return;value=Math.max(Number(input.min),Math.min(Number(input.max),value));if(event.type==='change'||!input.hasAttribute('data-lyric-number'))input.value=value;}state.project.settings[key]=value;if(['lyric_x','lyric_y'].includes(key))state.project.settings.position='custom';if(['gradient_start','gradient_end'].includes(key)&&state.project.settings.gradient_start>=state.project.settings.gradient_end)state.project.settings.gradient_end=Math.min(100,state.project.settings.gradient_start+1);renderStyle();dirty();if(key==='audio_normalize'&&$('exportDialog').open)refreshExportMenu().catch(handleError);};input.onchange=update;if(input.type==='range'||input.hasAttribute('data-lyric-number')){input.oninput=update;const finish=()=>delete input.dataset.historyActive;input.addEventListener('pointerup',finish);input.addEventListener('blur',finish);input.addEventListener('keyup',finish);}}
document.querySelectorAll('[data-lyric-slider]').forEach(slider=>{
  const input=$(slider.dataset.lyricSlider);
  slider.oninput=()=>{
    if(state.job)return;
    input.value=slider.value;
    input.dispatchEvent(new Event('input',{bubbles:true}));
  };
  slider.onchange=()=>input.dispatchEvent(new Event('change',{bubbles:true}));
  const finish=()=>delete input.dataset.historyActive;
  for(const event of ['pointerup','blur','keyup'])slider.addEventListener(event,finish);
});
const featureGroups=[...document.querySelectorAll('.style-panel>details,#wordTools')];featureGroups.forEach(group=>group.addEventListener('toggle',()=>{if(group.open)featureGroups.forEach(other=>{if(other!==group&&other.open)other.open=false;});}));
document.querySelectorAll('[data-title-y-step]').forEach(button=>button.onclick=()=>{const y=$('titleY');y.value=Math.max(+y.min,Math.min(+y.max,+y.value+Number(button.dataset.titleYStep)));y.dispatchEvent(new Event('input',{bubbles:true}));});
document.querySelectorAll('[data-title-x-step]').forEach(button=>button.onclick=()=>{const x=$('titleX');x.value=Math.max(+x.min,Math.min(+x.max,+x.value+Number(button.dataset.titleXStep)));x.dispatchEvent(new Event('input',{bubbles:true}));});
$('listenTrack').onchange=()=>{const position=beatNow();setAudio($('listenTrack').value,position);};$('playBtn').onclick=wrap(togglePlay);$('seek').oninput=()=>{audio.currentTime=+$('seek').value;previewFrame();drawTimeline();};$('playbackRate').onchange=()=>audio.playbackRate=+$('playbackRate').value;
audio.addEventListener('play',()=>{state.titlePreviewAt=null;$('playBtn').textContent='Ⅱ';document.body.classList.add('playing');});audio.addEventListener('pause',()=>{$('playBtn').textContent='▶';document.body.classList.remove('playing');});audio.addEventListener('error',()=>{if(audio.getAttribute('src'))toast('Không đọc được track. Hãy kiểm tra file hoặc chạy bước tách vocal trước.',true);});audio.addEventListener('timeupdate',()=>{previewFrame();drawTimeline();});
// Keep the lyric sweep on the display refresh rate. Redraw the editor canvases separately.
let lastPlaybackTimelineFrame = -Infinity;
function frame(now){
  if(!audio.paused||state.titlePreviewAt!=null){
    previewFrame();
    if(now-lastPlaybackTimelineFrame>=50){
      drawTimeline();
      lastPlaybackTimelineFrame=now;
    }
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
$('zoom').oninput=()=>{state.zoom=+$('zoom').value;drawTimeline();};$('pan').oninput=()=>{state.pan=+$('pan').value;drawTimeline();};new ResizeObserver(drawTimeline).observe($('timeline'));new ResizeObserver(()=>{if(state.project)renderStyle();}).observe($('preview'));
$('fullscreenBtn').onclick=wrap(()=>document.fullscreenElement?document.exitFullscreen():$('preview').requestFullscreen());
$('cancelJobBtn').onclick=wrap(async()=>{const job=state.job||state.renderJob;if(!job)return;await api(`/api/jobs/${job.id}/cancel`,{method:'POST',body:{}});$('jobTitle').textContent='Đang dừng tiến trình…';});
$('newBtn').onclick=wrap(newProject);
$('projectsBtn').onclick=wrap(async()=>{await saveNow();const projects=await api('/api/projects');$('projectList').innerHTML=projects.map(p=>`<div class="project-row"><label><input type="checkbox" data-batch-project="${p.id}" ${p.id===state.project.id?'checked':''}><span>Chọn</span></label><button class="project-item" data-project="${p.id}"><strong>${esc(p.name)}</strong><small>${new Date(p.updated_at*1000).toLocaleDateString('vi-VN')}</small><span>↗</span></button></div>`).join('');$('projectDialog').showModal();refreshBatchHistory().catch(handleError);});
$('projectList').onclick=wrap(async e=>{const button=e.target.closest('[data-project]');if(button){$('projectDialog').close();await openProject(button.dataset.project);}});
$('createVersionBtn').onclick=wrap(async()=>{await saveNow();const item=await api(endpoint('/versions'),{method:'POST',body:{note:'Lưu thủ công'}});toast('Đã lưu phiên bản '+new Date(item.created_at*1000).toLocaleString('vi-VN'));$('versionList').hidden=false;await refreshProjectVersions();});
async function refreshProjectVersions(){const versions=await api(endpoint('/versions'));$('versionList').innerHTML=versions.length?versions.map(item=>`<div class="version-item"><div><strong>${esc(item.name||'Project')}</strong><small>${new Date(item.created_at*1000).toLocaleString('vi-VN')} · ${esc(item.note||'Phiên bản đã lưu')}</small></div><button class="ghost" data-restore-version="${esc(item.id)}">Khôi phục</button></div>`).join(''):'<p class="hint">Chưa có phiên bản đã lưu.</p>';}
$('showVersionsBtn').onclick=wrap(async()=>{const list=$('versionList');list.hidden=!list.hidden;if(!list.hidden)await refreshProjectVersions();});
$('versionList').onclick=wrap(async event=>{const button=event.target.closest('[data-restore-version]');if(!button)return;if(!confirm('Khôi phục phiên bản này? Trạng thái hiện tại sẽ được lưu thành một phiên bản trước khi khôi phục.'))return;await saveNow();state.project=await api(endpoint(`/versions/${button.dataset.restoreVersion}/restore`),{method:'POST',body:{}});state.undo=[];state.redo=[];renderAll();setAudio(state.project.beat?'beat':'reference',0);updateHistoryButtons();await refreshProjectVersions();toast('Đã khôi phục phiên bản project.');});
$('queueBatchBtn').onclick=wrap(async()=>{const ids=[...document.querySelectorAll('#projectList [data-batch-project]:checked')].map(input=>input.dataset.batchProject);if(!ids.length)throw Error('Chọn ít nhất một project để dựng.');const aspects=['current'];if($('batchLandscapeOutput').checked)aspects.push('landscape');if($('batchPortraitOutput').checked)aspects.push('portrait');const includeGuide=$('batchGuideOutput').checked,includeBacking=$('batchBackingOutput').checked,maxOutputs=aspects.length*(1+Number(includeGuide)+Number(includeBacking));if(!confirm(`Đưa ${ids.length} project vào hàng đợi dựng? Mỗi project luôn có bản tỉ lệ hiện tại; có thể tạo tối đa ${maxOutputs} video/bài theo lựa chọn.`))return;await saveNow();const job=await api('/api/batches/render',{method:'POST',body:{project_ids:ids,aspects,include_guide:includeGuide,include_backing:includeBacking,require_review:$('batchRequireReview').checked,recognize_missing:$('batchRecognizeMissing').checked}});$('projectDialog').close();followJob(job);});
$('showBatchHistoryBtn').onclick=wrap(async()=>{const list=$('batchHistory');list.hidden=!list.hidden;if(!list.hidden)await refreshBatchHistory();});
async function refreshBatchHistory(){const batches=await api('/api/batches');$('batchHistory').innerHTML=batches.length?batches.map(batch=>`<div class="version-item"><div><strong>${new Date(batch.created_at*1000).toLocaleString('vi-VN')} · ${batch.completed}/${batch.count} xong</strong><small>${({queued:'Đang chờ',running:'Đang dựng',done:'Hoàn tất',cancelled:'Đã dừng',error:'Bị gián đoạn'})[batch.status]||batch.status} · lỗi ${batch.failed}</small></div>${(['cancelled','error'].includes(batch.status)||(batch.status==='done'&&batch.failed>0))?`<button class="ghost" data-resume-batch="${esc(batch.id)}">${batch.status==='done'?'Thử lại project lỗi':'Tiếp tục'}</button>`:''}</div>`).join(''):'<p class="hint">Chưa có hàng đợi dựng video.</p>';}
$('batchHistory').onclick=wrap(async event=>{const button=event.target.closest('[data-resume-batch]');if(!button)return;const job=await api(`/api/batches/${button.dataset.resumeBatch}/resume`,{method:'POST',body:{}});$('projectDialog').close();followJob(job);});
$('batchResultSummary').onclick=wrap(async event=>{const button=event.target.closest('[data-retry-batch]');if(!button)return;const job=await api(`/api/batches/${button.dataset.retryBatch}/resume`,{method:'POST',body:{}});$('batchResultDialog').close();followJob(job);});
$('closeBatchResults').onclick=()=>$('batchResultDialog').close();
document.querySelectorAll('[data-close]').forEach(button=>button.onclick=()=>button.closest('dialog').close());
$('backupBtn').onclick=wrap(async()=>{await saveNow();location.href=endpoint('/download/json');});
$('bundleLightBtn').onclick=wrap(async()=>{await saveNow();location.href=endpoint('/download/bundle-light');});
$('bundleFullBtn').onclick=wrap(async()=>{await saveNow();location.href=endpoint('/download/bundle-full');});
$('importProjectInput').onchange=wrap(async()=>{const file=$('importProjectInput').files[0];if(!file)return;await saveNow();let project;if(file.name.toLowerCase().endsWith('.zip')){const form=new FormData();form.append('file',file);project=await api('/api/projects/import-bundle',{method:'POST',body:form});}else{const data=JSON.parse((await file.text()).replace(/^\uFEFF/,''));project=await api('/api/projects/import',{method:'POST',body:data});}$('projectDialog').close();await openProject(project.id);$('importProjectInput').value='';toast(file.name.toLowerCase().endsWith('.zip')?'Đã nhập project ZIP.':'Đã nhập lời và mốc; nạp lại bản hát/beat nếu dùng bản JSON.');});
$('exportBtn').onclick=wrap(async()=>{await saveNow();await refreshExportMenu();$('exportDialog').showModal();});
$('previewRenderBtn').onclick=wrap(async()=>{await saveNow();if(!state.project.beat)throw Error('Hãy nạp beat gốc trước.');const total=Number(state.project.beat.duration)||0,start=Math.max(0,Math.min(Math.max(0,total-.5),beatNow())),duration=Math.min(8,total-start);$('renderResult').hidden=true;$('downloadVideo').hidden=false;$('exportDialog').close();const result=await api(endpoint('/render'),{method:'POST',body:{preview:{start,duration}}}),job=await followJob(result,{previewRender:true});if(job?.result?.preview){$('renderResult').querySelector('strong').textContent='Đoạn render thử đã sẵn sàng';$('downloadVideo').hidden=true;$('exportSavedPath').textContent=`Bản thử ${job.result.preview_duration} giây · bắt đầu tại ${time(job.result.preview_start)}`;$('renderQaContact').hidden=true;}});$('renderBtn').onclick=wrap(async()=>{await saveNow();if(!state.project.beat)throw Error('Hãy nạp beat gốc trước.');$('renderResult').hidden=true;$('downloadVideo').hidden=false;$('exportDialog').close();followJob(await api(endpoint('/render'),{method:'POST',body:{}}));});
document.querySelectorAll('[data-download]').forEach(button=>button.onclick=wrap(async()=>{await saveNow();location.href=endpoint(`/download/${button.dataset.download}`);}));
document.querySelectorAll('[data-subtitle-export]').forEach(button=>button.onclick=wrap(async()=>{if(!state.project.words.length)throw Error('Căn lời trước khi xuất phụ đề.');await saveNow();location.href=endpoint(`/download/${button.dataset.subtitleExport}`);button.closest('.menu-pop')?.removeAttribute('open');}));
$('importSrtBtn').onclick=()=>$('srtInput').click();
$('srtInput').onchange=wrap(async()=>{const file=$('srtInput').files[0];if(!file)return;const text=(await file.text()).replace(/^\uFEFF/,'').replace(/\r/g,'');const blocks=text.trim().split(/\n\s*\n/),words=[];let li=0;function sec(h,m,s,ms){return +h*3600 + +m*60 + +s + +ms/1000;}for(const block of blocks){const lines=block.split('\n');const index=lines.findIndex(l=>l.includes('-->'));if(index<0)continue;const match=lines[index].match(/(\d+):(\d+):(\d+)[,.](\d{3})\s*-->\s*(\d+):(\d+):(\d+)[,.](\d{3})/);if(!match)continue;const a=sec(...match.slice(1,5)),b=sec(...match.slice(5,9));const tokens=lines.slice(index+1).join(' ').replace(/<[^>]+>/g,'').split(/\s+/).filter(Boolean);tokens.forEach((token,k)=>{const start=mapTime(a+(b-a)*k/tokens.length,true),end=mapTime(a+(b-a)*(k+1)/tokens.length,true);if(start>=0&&end>start)words.push({text:token,start,end,line:li,role:'all',estimated:true});});li++;}if(!words.length)throw Error('Không đọc được câu nào từ SRT.');if(state.project.words.some(word=>word.locked))throw Error('Mở khóa các câu đã khóa trước khi thay toàn bộ lời bằng SRT.');if(state.project.words.length&&!confirm('Nhập SRT sẽ thay lời và mốc từ hiện tại. Tiếp tục?'))return;remember();state.project.words=words;const grouped=new Map();for(const word of words){if(!grouped.has(word.line))grouped.set(word.line,[]);grouped.get(word.line).push(word.text);}state.project.text=[...grouped.values()].map(line=>line.join(' ')).join('\n');$('lyricsText').value=state.project.text;renderWords();drawTimeline();await saveNow();$('srtInput').value='';toast('Đã nhập SRT theo thời gian beat. Mốc từng từ được chia ước lượng trong mỗi câu.');});
document.addEventListener('keydown',wrap(async event=>{const typing=['INPUT','TEXTAREA','SELECT'].includes(document.activeElement?.tagName);if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='s'){event.preventDefault();await saveNow();toast('Đã lưu project trên máy.');}else if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='z'&&event.shiftKey&&!typing){event.preventDefault();redo();}else if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='z'&&!typing){event.preventDefault();undo();}else if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='y'&&!typing){event.preventDefault();redo();}else if(event.code==='Space'&&!typing&&!document.querySelector('dialog[open]')){event.preventDefault();await togglePlay();}}));
window.addEventListener('beforeunload',event=>{if(state.saveTimer||state.job||state.manualTiming){event.preventDefault();event.returnValue='';}});

async function start(){const health=await api('/api/health');$('dataStoragePath').textContent=health.data_directory||'Chưa xác định';$('healthStatus').textContent=health.ok?'● Môi trường sẵn sàng':'Thiếu: '+Object.entries(health.dependencies).filter(([,ok])=>!ok).map(([key])=>key).join(', ');const projects=await api('/api/projects');const requested=new URLSearchParams(location.search).get('project');const last=requested||localStorage.getItem('obs_project');if(projects.some(p=>p.id===last))await openProject(last);else if(projects.length)await openProject(projects[0].id);else await newProject();await loadPresets();scanHardware().catch(error=>{console.error(error);});initUpdates();}
initStudio();
initTapping();
start().catch(handleError);

async function scanHardware(refresh=false){
  const status=$('aiHardwareStatus');
  if(status)status.textContent='Đang kiểm tra thiết bị…';
  try{
    const h=await api('/api/hardware'+(refresh?'?refresh=1':''));
    const cards=(h.gpus||[]).map(g=>esc(g.name)+' · '+Math.round(g.memory_mb/1024)+' GB').join(', ');
    const backends=['demucs','whisper'].map(k=>h[k]?.device?.startsWith('cuda')?'GPU':'CPU');
    const text=(cards||'Không phát hiện NVIDIA GPU')+' · Tự động: '+[...new Set(backends)].join('/')+' · Video: '+(h.video?.encoder==='h264_nvenc'?'GPU':'CPU');
    if(status){status.textContent=text;status.title=['demucs','whisper','video'].filter(k=>h[k]?.reason).map(k=>k+': '+h[k].reason).join('\n');}
  }catch(error){if(status)status.textContent='Chưa kiểm tra được thiết bị; app sẽ tự thử lại khi xử lý.';throw error;}
}

$('aiModelBtn').onclick=wrap(async()=>{
  $('aiModel').value=selectedAiModel();$('aiLanguage').value=selectedAiLanguage();
  await scanHardware();$('aiModelDialog').showModal();
});
$('saveAiModel').onclick=wrap(()=>{
  localStorage.setItem(AI_MODEL_STORAGE,$('aiModel').value);localStorage.setItem(AI_LANGUAGE_STORAGE,$('aiLanguage').value);
  updateAiButton();$('aiModelDialog').close();toast('Đã lưu model AI. CPU/GPU sẽ tự động chọn.');
});
updateAiButton();

async function loadPresets(){state.presets=await api('/api/presets');renderPresetGrid();renderStyleControls();renderInlinePresets();}
function renderPresetGrid(){const q=$('presetSearch').value.toLocaleLowerCase('vi');$('presetGrid').innerHTML=state.presets.filter(p=>p.name.toLocaleLowerCase('vi').includes(q)).map(p=>`<button class="preset-card ${p.id===state.presetId?'selected':''}" data-preset="${esc(p.id)}"><span class="preset-colors"><i style="background:${p.settings.sung}"></i><i style="background:${p.settings.unsung}"></i><i style="background:${p.settings.outline_unsung}"></i></span><strong>${esc(p.name)}</strong><small>${p.builtin?'VCL · có sẵn':'Mẫu riêng'}</small></button>`).join('')||'<p class="hint">Không tìm thấy preset.</p>';}
function showPreset(id){const p=state.presets.find(p=>p.id===id);state.presetId=p?.id||null;renderPresetGrid();$('presetDetail').hidden=!p;if(!p)return;$('presetName').textContent=p.name;$('presetDescription').textContent=p.description;$('presetNotes').textContent=p.notes.join(' ');$('deletePresetBtn').hidden=p.builtin;const s={...state.project.settings,...p.settings},sample=$('presetSample');sample.style.fontFamily=s.font;sample.style.fontStyle=s.italic?'italic':'normal';sample.style.backgroundColor=s.background;const parts=sample.querySelectorAll('span');parts[0].textContent=sourceFontText('Hát cùng ',s.font);parts[1].textContent=sourceFontText('bạn hôm nay',s.font);parts[0].style.color=s.sung;parts[1].style.color=s.unsung;for(let i=0;i<2;i++)parts[i].style.webkitTextStroke=`${s.outline_width*.35}px ${colorAlpha(s[i?'outline_unsung':'outline_sung'],s[i?'outline_unsung_opacity':'outline_sung_opacity'])}`;sample.style.textShadow=s.shadow?`${s.shadow*.35}px ${s.shadow*.35}px ${s.shadow*.35}px ${s.shadow_color}`:'none';}
$('presetSearch').oninput=renderPresetGrid;
$('presetGrid').onclick=event=>{const button=event.target.closest('[data-preset]');if(button)showPreset(button.dataset.preset);};
$('applyPresetBtn').onclick=wrap(()=>applyPresetNow(state.presetId));
$('savePresetBtn').onclick=wrap(async()=>{const name=$('newPresetName').value.trim();if(!name){$('newPresetName').focus();throw Error('Nhập tên cho preset riêng.');}const p=await api('/api/presets',{method:'POST',body:{format:'original-beat-preset',version:1,name,description:'Thiết lập đã lưu từ project',settings:state.project.settings}});await loadPresets();showPreset(p.id);$('newPresetName').value='';toast('Đã lưu preset riêng.');});
$('exportPresetBtn').onclick=()=>{if(state.presetId)location.href=`/api/presets/${encodeURIComponent(state.presetId)}/download`;};
$('deletePresetBtn').onclick=wrap(async()=>{const p=state.presets.find(p=>p.id===state.presetId);if(!p||p.builtin)return;await api(`/api/presets/${p.id}`,{method:'DELETE'});state.presetId=null;await loadPresets();showPreset(state.presets[0]?.id);toast('Đã xóa preset riêng.');});
$('importPresetInput').onchange=wrap(async()=>{const input=$('importPresetInput'),file=input.files[0];if(!file)return;try{if(file.size>1024*1024)throw Error('Preset JSON tối đa 1 MB.');const data=JSON.parse((await file.text()).replace(/^\uFEFF/,''));const p=await api('/api/presets',{method:'POST',body:data});await loadPresets();showPreset(p.id);toast('Đã nhập '+p.name+'. Bấm Áp dụng để dùng.');}finally{input.value='';}});

(() => {
  const openBtn = $('searchLyricsOnlineBtn'), dlg = $('lyricsSearchDialog');
  if (!openBtn || !dlg) return;
  let selectedText = '';
  const setButtons = on => {
    $('copyOnlineLyricsBtn').disabled = !on;
    $('applyOnlineLyricsBtn').disabled = !on;
  };
  openBtn.onclick = () => {
    const title = ($('projectName')?.value || '').trim();
    if (!$('lyricsSearchQuery').value.trim() && title && title !== 'Bài karaoke mới') {
      $('lyricsSearchQuery').value = title;
    }
    $('lyricsCurrentCompare').value = $('lyricsText')?.value || state.project?.text || '';
    $('lyricsSearchPreview').value = '';
    $('lyricsSearchResults').innerHTML = '';
    setButtons(false);
    dlg.showModal();
    if ($('lyricsSearchQuery').value.trim()) $('lyricsSearchForm').requestSubmit();
    else $('lyricsSearchQuery').focus();
  };
  $('lyricsSearchForm').onsubmit = wrap(async e => {
    e.preventDefault();
    const q = $('lyricsSearchQuery').value.trim();
    if (!q) return;
    $('lyricsSearchStatus').textContent = 'Đang tìm lời trên mạng…';
    $('lyricsSearchResults').innerHTML = '';
    $('lyricsSearchPreview').value = '';
    setButtons(false);
    const data = await api('/api/lyrics/search-web', { method: 'POST', body: { query: q } });
    const list = data.results || [];
    $('lyricsSearchStatus').textContent = list.length ? `Tìm thấy ${list.length} kết quả. Chọn một bài để đối chiếu:` : 'Không tìm thấy kết quả phù hợp.';
    for (const item of list) {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'ghost small';
      btn.style.cssText = 'text-align:left;justify-content:flex-start;flex-direction:column;align-items:flex-start;gap:2px;padding:8px;height:auto;white-space:normal';
      const dur = item.duration ? ` · ${Math.floor(item.duration / 60)}:${String(Math.round(item.duration % 60)).padStart(2, '0')}` : '';
      const src = item.source ? `[${esc(item.source)}] ` : '';
      btn.innerHTML = `<strong style="font-size:12px">${esc(item.trackName || 'Không rõ tên')}</strong><span class="small muted">${src}${esc(item.artistName || 'Không rõ ca sĩ')}${dur}</span>`;
      btn.onclick = () => {
        for (const b of $('lyricsSearchResults').children) b.classList.remove('on');
        btn.classList.add('on');
        selectedText = (item.plainLyrics || item.syncedLyrics.replace(/\[[^\]]+\]\s*/g, '')).trim();
        $('lyricsSearchPreview').value = selectedText;
        setButtons(!!selectedText);
      };
      $('lyricsSearchResults').appendChild(btn);
    }
    if (list.length) $('lyricsSearchResults').firstElementChild?.click();
  });
  $('copyOnlineLyricsBtn').onclick = () => {
    if (!selectedText) return;
    $('lyricsText').value = selectedText;
    $('lyricsEditor').open = true;
    dlg.close();
    toast('Đã chép lời vào ô sửa lời.');
  };
  $('applyOnlineLyricsBtn').onclick = () => {
    if (!selectedText) return;
    $('lyricsText').value = selectedText;
    dlg.close();
    $('applyLyricsEditsBtn').click();
  };
})();

(() => {
  const wrapEl = document.querySelector('.transport-quick');
  if (!wrapEl) return;
  const notes = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
  let pitchReqId = 0, pitchDebounce = null;
  function showPitchProgress(st) {
    const jb = $('jobBar'), jt = $('jobTitle'), jp = $('jobProgress'), jpct = $('jobPercent');
    if (!jb) return () => {};
    jb.hidden = false;
    if (jt) jt.textContent = `Đang xử lý chuyển tone (${st > 0 ? '+' : ''}${st} st)…`;
    let pct = 15;
    if (jp) jp.style.width = pct + '%';
    if (jpct) jpct.textContent = pct + '%';
    const iv = setInterval(() => {
      pct = Math.min(92, pct + 8);
      if (jp) jp.style.width = pct + '%';
      if (jpct) jpct.textContent = pct + '%';
    }, 250);
    return () => {
      clearInterval(iv);
      if (jp) jp.style.width = '100%';
      if (jpct) jpct.textContent = '100%';
      setTimeout(() => { if (!state.job) jb.hidden = true; }, 200);
    };
  }
  function applyPitchToAudio() {
    if (!state.project) return;
    const st = Math.round(Number(state.project?.settings?.pitch_semitones || 0));
    const baseRate = Number($('playbackRate')?.value || 1);
    audio.preservesPitch = true; audio.playbackRate = baseRate;
    const curSrc = audio.getAttribute('src');
    if (!curSrc) return;
    const base = curSrc.replace(/&pitch=[^&]+/g, '');
    const want = st !== 0 ? `${base}&pitch=${st}` : base;
    if (curSrc === want) { if (typeof syncAuxTracks === 'function') syncAuxTracks(); return; }
    clearTimeout(pitchDebounce);
    const reqId = ++pitchReqId;
    pitchDebounce = setTimeout(async () => {
      const pos = audio.currentTime || 0, wasPlaying = !audio.paused;
      const done = st !== 0 ? showPitchProgress(st) : () => {};
      try {
        if (st !== 0) {
          await fetch(want, { method: 'HEAD' });
          const vocSlot = state.project.vocals_ready ? 'vocals' : 'reference';
          for (const extra of [vocSlot, 'backing']) {
            if (state.trackMuted && state.trackMuted[extra]) continue;
            const u = endpoint(`/audio/${extra}`) + `?v=${state.project.updated_at}&pitch=${st}`;
            if (u !== want) await fetch(u, { method: 'HEAD' }).catch(() => {});
          }
        }
        if (reqId !== pitchReqId) return;
        audio.onloadeddata = () => {
          audio.onloadeddata = null;
          audio.playbackRate = baseRate;
          if (pos > 0.05) try { audio.currentTime = Math.min(audio.duration || pos, pos); } catch {}
          if (wasPlaying) audio.play().catch(() => {});
        };
        audio.src = want;
        if (typeof syncAuxTracks === 'function') syncAuxTracks();
      } finally {
        done();
      }
    }, 220);
  }
  function syncTq() {
    const p = state.project, s = p?.settings || {};
    const off = Number(s.sync_offset || 0);
    const st = Math.round(Number(s.pitch_semitones || 0));
    $('tqOffset').textContent = (off >= 0 ? '+' : '') + off.toFixed(1) + 's';
    if ($('tqPitch')) {
      const targetNote = notes[((st % 12) + 12) % 12];
      $('tqPitch').textContent = st === 0 ? '0 st' : `${st > 0 ? '+' : ''}${st} (${targetNote})`;
    }
    $('tqFirstLead').textContent = Number(s.lead_in ?? 5).toFixed(1) + 's';
    $('tqNextLead').textContent = Number(s.row_lead ?? 1.5).toFixed(1) + 's';
  }
  wrapEl.addEventListener('click', wrap(async e => {
    const btn = e.target.closest('[data-tq]');
    if (!btn || !state.project) return;
    const s = state.project.settings;
    const key = btn.dataset.tq, delta = Number(btn.dataset.delta || 0);
    if (key === 'offset') {
      const next = Math.max(-60, Math.min(60, Math.round((Number(s.sync_offset || 0) + delta) * 10) / 10));
      const actualDelta = next - Number(s.sync_offset || 0);
      if (!actualDelta) return;
      remember();
      s.sync_offset = next;
      const dur = beatDuration() || 86400;
      for (const w of state.project.words) {
        if (lineLocked(w.line)) continue;
        const ns = Math.max(0, Math.min(dur - 0.05, w.start + actualDelta));
        const ne = Math.max(ns + 0.05, Math.min(dur, w.end + actualDelta));
        w.start = ns; w.end = ne;
      }
      syncTq(); renderWords(); renderSelected(); drawTimeline(); scheduleLayout(); previewFrame(); dirty();
    } else if (key === 'pitch') {
      const next = Math.max(-12, Math.min(12, Math.round(Number(s.pitch_semitones || 0) + delta)));
      s.pitch_semitones = next;
      applyPitchToAudio();
      syncTq(); dirty();
    } else if (key === 'first_lead') {
      const next = Math.max(0, Math.min(15, Math.round((Number(s.lead_in ?? 5) + delta) * 10) / 10));
      s.lead_in = next;
      syncTq(); scheduleLayout(); previewFrame(); dirty();
    } else if (key === 'next_lead') {
      const next = Math.max(0, Math.min(8, Math.round((Number(s.row_lead ?? 1.5) + delta) * 10) / 10));
      s.row_lead = next;
      syncTq(); scheduleLayout(); previewFrame(); dirty();
    }
  }));
  $('playbackRate')?.addEventListener('change', applyPitchToAudio);
  setInterval(syncTq, 600);
})();

(() => {
  const qsScreen = $('karaokeStartScreen'), page = $('karaokePage');
  if (!qsScreen || !page) return;
  const skipped = new Set();
  let qsMode = 'file', qsFile = null;
  const qsAudio = new Audio();
  let qsPeaks = [];

  function fmtDur(sec) {
    if (!Number.isFinite(sec) || sec <= 0) return '0:00';
    return `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, '0')}`;
  }
  function drawQsWave() {
    const c = $('qsWaveCanvas');
    if (!c) return;
    const W = c.clientWidth || 680, H = c.height || 54;
    if (c.width !== W) c.width = W;
    const ctx = c.getContext('2d');
    ctx.clearRect(0, 0, W, H);
    const prog = qsAudio.duration ? qsAudio.currentTime / qsAudio.duration : 0;
    const n = qsPeaks.length || 120;
    for (let x = 0; x < W; x += 3) {
      const idx = Math.floor((x / W) * n);
      const v = qsPeaks.length ? qsPeaks[idx] : (0.25 + 0.55 * Math.abs(Math.sin(x * 0.07) * Math.cos(x * 0.023)));
      const h = Math.max(4, v * (H - 8));
      ctx.fillStyle = (x / W) <= prog ? '#f97316' : '#4b5563';
      ctx.fillRect(x, (H - h) / 2, 2, h);
    }
    const cx = prog * W;
    ctx.fillStyle = '#ef4444';
    ctx.fillRect(cx, 0, 2, H);
  }
  qsAudio.ontimeupdate = () => {
    $('qsFileDur').textContent = `${fmtDur(qsAudio.currentTime)} / ${fmtDur(qsAudio.duration)}`;
    drawQsWave();
  };
  qsAudio.onended = () => { $('qsPlayBtn').textContent = '▶'; };

  function checkQsVisibility() {
    const p = state.project;
    if (!p) return;
    const hasMedia = !!(p.beat || p.reference || p.vocals_ready);
    const show = !hasMedia && !skipped.has(p.id) && !state.job;
    qsScreen.hidden = !show;
    page.classList.toggle('qs-active', show);
    if (!show && !qsAudio.paused) qsAudio.pause();
  }
  setInterval(checkQsVisibility, 400);

  qsScreen.addEventListener('click', e => {
    const tab = e.target.closest('[data-qs-mode]');
    if (!tab) return;
    qsMode = tab.dataset.qsMode;
    for (const b of qsScreen.querySelectorAll('[data-qs-mode]')) {b.classList.toggle('on', b === tab);b.setAttribute('aria-selected',String(b===tab));}
    $('qsFileBar').hidden = qsMode !== 'file';
    $('qsYoutubeBar').hidden = qsMode !== 'youtube';
    $('qsPreviewCard').hidden = qsMode !== 'file' || !qsFile;
    $('qsOptionsBlock').hidden = qsMode === 'file' ? !qsFile : !$('qsYoutubeInput').value.trim();
  });

  $('qsBrowseBtn').onclick = () => $('qsFileInput').click();
  $('qsFileLabel').onclick = () => $('qsFileInput').click();
  $('qsFileInput').onchange = async () => {
    const f = $('qsFileInput').files[0];
    if (!f) return;
    qsFile = f;
    $('qsFileLabel').value = f.name;
    $('qsFileName').textContent = f.name;
    $('qsPreviewCard').hidden = false;
    $('qsOptionsBlock').hidden = false;
    qsAudio.src = URL.createObjectURL(f);
    qsAudio.onloadedmetadata = () => {
      $('qsFileDur').textContent = `0:00 / ${fmtDur(qsAudio.duration)}`;
      drawQsWave();
    };
    try {
      const buf = await f.arrayBuffer();
      const ac = new (window.AudioContext || window.webkitAudioContext)();
      const decoded = await ac.decodeAudioData(buf);
      const ch = decoded.getChannelData(0);
      const bins = 220, step = Math.max(1, Math.floor(ch.length / bins));
      qsPeaks = [];
      for (let i = 0; i < bins; i++) {
        let max = 0;
        for (let j = 0; j < step; j += 16) {
          const val = Math.abs(ch[i * step + j] || 0);
          if (val > max) max = val;
        }
        qsPeaks.push(Math.min(1, max));
      }
      ac.close();
      drawQsWave();
    } catch {
      qsPeaks = [];
      drawQsWave();
    }
  };
  $('qsYoutubeInput').oninput = () => {
    $('qsOptionsBlock').hidden = !$('qsYoutubeInput').value.trim();
  };
  $('qsPlayBtn').onclick = () => {
    if (!qsAudio.src) return;
    if (qsAudio.paused) { qsAudio.play(); $('qsPlayBtn').textContent = '⏸'; }
    else { qsAudio.pause(); $('qsPlayBtn').textContent = '▶'; }
  };
  $('qsVolSlider').oninput = e => { qsAudio.volume = Number(e.target.value); };
  $('qsWaveCanvas').onclick = e => {
    if (!qsAudio.duration) return;
    const r = e.currentTarget.getBoundingClientRect();
    qsAudio.currentTime = Math.max(0, Math.min(qsAudio.duration, ((e.clientX - r.left) / r.width) * qsAudio.duration));
    drawQsWave();
  };
  $('qsSkipBtn').onclick = () => {
    if (state.project) skipped.add(state.project.id);
    checkQsVisibility();
  };
  $('qsStartBtn').onclick = wrap(async () => {
    if (!state.project) return;
    if (!qsAudio.paused) qsAudio.pause();
    const hasLyrics = qsScreen.querySelector('input[name="qsHasLyrics"]:checked')?.value !== '0';
    const aiModel = $('quickWhisperModel')?.value || $('aiModel')?.value || 'medium';
    const aiLang = $('aiLanguage')?.value || 'vi';
    const aiDevice = $('quickAiDevice')?.value || 'auto';
    const backRaw = $('quickBackingModel')?.value || 'backing-roformer';
    const backModel = { 'backing-roformer': 'roformer', 'mdx-kara-2': 'mdx-kara-2', 'backing-uvr-lite': 'uvr-lite' }[backRaw] || 'roformer';
    skipped.add(state.project.id);
    checkQsVisibility();
    let job;
    if (qsMode === 'youtube') {
      const url = $('qsYoutubeInput').value.trim();
      if (!url) throw Error('Hãy nhập link YouTube.');
      job = await api(endpoint('/quick-start'), { method: 'POST', body: { youtube_url: url, has_lyrics: hasLyrics, model: aiModel, language: aiLang, device: aiDevice, backing_model: backModel } });
    } else {
      if (!qsFile) throw Error('Hãy chọn file nhạc hoặc video.');
      const fd = new FormData();
      fd.append('file', qsFile);
      fd.append('has_lyrics', hasLyrics ? '1' : '0');
      fd.append('model', aiModel);
      fd.append('language', aiLang);
      fd.append('device', aiDevice);
      fd.append('backing_model', backModel);
      job = await api(endpoint('/quick-start'), { method: 'POST', body: fd });
    }
    await followJob(job);
    const fresh = await api(endpoint(''));
    state.project = fresh;
    renderAll();
  });
})();

(() => {
  // 1. Tự động đóng menu Tệp khi bấm ra ngoài hoặc bấm chọn mục trong menu
  document.addEventListener('pointerdown', e => {
    for (const d of document.querySelectorAll('details.menu-pop[open]')) {
      if (!d.contains(e.target)) d.open = false;
    }
  });
  document.addEventListener('click', e => {
    const inDrop = e.target.closest('.menu-drop button');
    if (inDrop) {
      for (const d of document.querySelectorAll('details.menu-pop[open]')) d.open = false;
    }
  });

  // 2. Nút Lưu project trong menu Tệp
  const saveMenuBtn = $('menuSaveBtn');
  if (saveMenuBtn) {
    saveMenuBtn.onclick = wrap(async () => {
      await saveNow();
      toast('Đã lưu project.');
    });
  }

  // 3. Đồng bộ Thiết bị AI & Chọn nhanh Model trên thanh công cụ
  const devSel = $('quickAiDevice');
  const instSel = $('quickInstModel');
  const backSel = $('quickBackingModel');
  const whispSel = $('quickWhisperModel');

  function syncQuickSelectorsFromStorage() {
    const dev = localStorage.getItem('obs_ai_device') || 'auto';
    const inst = localStorage.getItem('obs_active_inst_model') || 'mdx-inst-hq-1';
    const back = localStorage.getItem('obs_active_backing_model') || 'backing-roformer';
    const whisp = localStorage.getItem('obs_active_whisper_model') || $('aiModel')?.value || 'medium';
    if (devSel) devSel.value = dev;
    if (instSel) instSel.value = inst;
    if (backSel) backSel.value = back;
    if (whispSel) whispSel.value = whisp;
    if ($('aiModel')) $('aiModel').value = whisp;
    if ($('separationInstModel')) $('separationInstModel').value = inst;
    if ($('separationBackingModel')) {
      $('separationBackingModel').value = { 'backing-roformer': 'roformer', 'mdx-kara-2': 'mdx-kara-2', 'backing-uvr-lite': 'uvr-lite' }[back] || 'roformer';
    }
  }
  syncQuickSelectorsFromStorage();
  setInterval(syncQuickSelectorsFromStorage, 1000);

  devSel?.addEventListener('change', () => {
    localStorage.setItem('obs_ai_device', devSel.value);
    toast('Thiết bị xử lý AI: ' + devSel.options[devSel.selectedIndex].text);
  });
  instSel?.addEventListener('change', () => {
    localStorage.setItem('obs_active_inst_model', instSel.value);
    if ($('separationInstModel')) $('separationInstModel').value = instSel.value;
    if (typeof refreshModels === 'function' && !$('settingsPage')?.hidden) refreshModels();
    toast('Model tách nhạc: ' + instSel.options[instSel.selectedIndex].text);
  });
  backSel?.addEventListener('change', () => {
    localStorage.setItem('obs_active_backing_model', backSel.value);
    if ($('separationBackingModel')) {
      $('separationBackingModel').value = { 'backing-roformer': 'roformer', 'mdx-kara-2': 'mdx-kara-2', 'backing-uvr-lite': 'uvr-lite' }[backSel.value] || 'roformer';
    }
    if (typeof refreshModels === 'function' && !$('settingsPage')?.hidden) refreshModels();
    toast('Model tách bè: ' + backSel.options[backSel.selectedIndex].text);
  });
  whispSel?.addEventListener('change', () => {
    localStorage.setItem('obs_active_whisper_model', whispSel.value);
    if ($('aiModel')) $('aiModel').value = whispSel.value;
    if (typeof refreshModels === 'function' && !$('settingsPage')?.hidden) refreshModels();
    toast('Model nhận lời: ' + whispSel.options[whispSel.selectedIndex].text);
  });
})();

(() => {
  // 1. Giao diện gọn cho trang Tách nhạc lấy beat (chọn nguồn xong mới hiện 2 nút tách)
  const sepPage = $('separationPage');
  if (sepPage) {
    let sepMode = 'file';
    function updateSepChoices() {
      const hasFile = !!$('separationInput')?.files?.length;
      const hasUrl = !!$('separationYoutubeUrl')?.value?.trim();
      if ($('sepChoicesWrap')) {
        $('sepChoicesWrap').hidden = sepMode === 'file' ? !hasFile : !hasUrl;
      }
    }
    sepPage.addEventListener('click', e => {
      const tab = e.target.closest('[data-sep-tab]');
      if (!tab) return;
      sepMode = tab.dataset.sepTab;
      for (const b of sepPage.querySelectorAll('[data-sep-tab]')) {b.classList.toggle('on', b === tab);b.setAttribute('aria-selected',String(b === tab));}
      if ($('sepFileBar')) $('sepFileBar').hidden = sepMode !== 'file';
      if ($('sepYoutubeBar')) $('sepYoutubeBar').hidden = sepMode !== 'youtube';
      if (sepMode === 'file') $('separationYoutubeUrl').value = '';
      updateSepChoices();
    });
    $('sepBrowseBtn')?.addEventListener('click', () => $('separationInput')?.click());
    $('sepFileDisplay')?.addEventListener('click', () => $('separationInput')?.click());
    $('separationInput')?.addEventListener('change', () => {
      const f = $('separationInput')?.files?.[0];
      if ($('sepFileDisplay')) $('sepFileDisplay').value = f ? f.name : '';
      updateSepChoices();
    });
    $('separationYoutubeUrl')?.addEventListener('input', updateSepChoices);
  }

  // 2. Trình phát hiện đại (.modern-audio-player) & Nâng / Hạ Tone sau khi tách
  let sepPitch = 0;
  const baseDownloadHrefs = new Map();

  function fmtMapTime(sec) {
    if (!Number.isFinite(sec) || sec < 0) return '0:00';
    return `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, '0')}`;
  }

  let sepPitchReq = 0;
  function syncAudioPitchSrc(aud, wantPlay) {
    if (!aud) return false;
    const cur = aud.getAttribute('src');
    if (!cur) return false;
    const base = cur.replace(/&pitch=[^&]+/g, '');
    const want = sepPitch === 0 ? base : `${base}&pitch=${sepPitch}`;
    if (cur === want) return false;
    const pos = aud.currentTime || 0;
    const reqId = ++sepPitchReq;
    const jb = $('jobBar'), jt = $('jobTitle'), jp = $('jobProgress'), jpct = $('jobPercent');
    if (jb && sepPitch !== 0) {
      jb.hidden = false;
      if (jt) jt.textContent = `Đang xử lý chuyển tone (${sepPitch > 0 ? '+' : ''}${sepPitch} st)…`;
      if (jp) jp.style.width = '45%';
      if (jpct) jpct.textContent = '45%';
    }
    if ($('sepPitchValue')) $('sepPitchValue').textContent = '⏳ Đang xử lý…';
    fetch(want, { method: 'HEAD' }).finally(() => {
      if (reqId !== sepPitchReq) return;
      if (jb && !state.job) jb.hidden = true;
      if ($('sepPitchValue')) {
        $('sepPitchValue').textContent = sepPitch === 0 ? '0 st (Gốc)' : `${sepPitch > 0 ? '+' : ''}${sepPitch} st (R3)`;
      }
      aud.onloadeddata = () => {
        aud.onloadeddata = null;
        if (pos > 0.05) {
          try { aud.currentTime = Math.min(aud.duration || pos, pos); } catch {}
        }
        if (wantPlay) aud.play().catch(() => {});
      };
      aud.src = want;
    });
    return true;
  }

  function applySepPitchToPlayers() {
    if ($('sepPitchValue')) {
      $('sepPitchValue').textContent = sepPitch === 0 ? '0 st (Gốc)' : `${sepPitch > 0 ? '+' : ''}${sepPitch} st (R3)`;
    }
    for (const b of document.querySelectorAll('[data-sep-pitch-set]')) {
      b.classList.toggle('on', Number(b.dataset.sepPitchSet) === sepPitch);
    }
    for (const tag of document.querySelectorAll('.map-pitch-tag')) {
      tag.textContent = `Tone: ${sepPitch > 0 ? '+' : ''}${sepPitch} st`;
    }
    for (const id of ['separatedBeatPreview', 'separatedBeatBackingPreview', 'separatedVocalPreview']) {
      const aud = $(id);
      if (!aud || aud.paused) continue;
      syncAudioPitchSrc(aud, true);
    }
    for (const dlId of ['separatedBeatDownload', 'separatedBeatBackingDownload', 'separatedVocalDownload']) {
      const a = $(dlId);
      if (!a) continue;
      const cur = a.getAttribute('href');
      if (!cur) continue;
      const clean = cur.split('?')[0];
      baseDownloadHrefs.set(dlId, clean);
      a.setAttribute('href', sepPitch === 0 ? clean : `${clean}?pitch=${sepPitch}`);
    }
  }

  document.addEventListener('click', e => {
    const stepBtn = e.target.closest('[data-sep-pitch-delta]');
    if (stepBtn) {
      sepPitch = Math.max(-12, Math.min(12, sepPitch + Number(stepBtn.dataset.sepPitchDelta || 0)));
      applySepPitchToPlayers();
      return;
    }
    const setBtn = e.target.closest('[data-sep-pitch-set]');
    if (setBtn) {
      sepPitch = Math.max(-12, Math.min(12, Number(setBtn.dataset.sepPitchSet || 0)));
      applySepPitchToPlayers();
      return;
    }
  });

  for (const wrapEl of document.querySelectorAll('.modern-audio-player[data-for-audio]')) {
    const aud = $(wrapEl.dataset.forAudio);
    if (!aud) continue;
    const playBtn = wrapEl.querySelector('.map-play');
    const seekEl = wrapEl.querySelector('.map-seek');
    const timeEl = wrapEl.querySelector('.map-time');
    const volEl = wrapEl.querySelector('.map-vol');

    playBtn.onclick = () => {
      if (!aud.getAttribute('src')) return toast('Chưa có bản âm thanh này.');
      if (aud.paused) {
        for (const otherId of ['separatedBeatPreview', 'separatedBeatBackingPreview', 'separatedVocalPreview']) {
          const o = $(otherId);
          if (o && o !== aud && !o.paused) o.pause();
        }
        if (!syncAudioPitchSrc(aud, true)) {
          aud.play().catch(err => { if (err?.name !== 'AbortError') handleError(err); });
        }
      } else {
        aud.pause();
      }
    };
    aud.addEventListener('play', () => { playBtn.textContent = '⏸'; });
    aud.addEventListener('pause', () => { playBtn.textContent = '▶'; });
    aud.addEventListener('ended', () => { playBtn.textContent = '▶'; });
    aud.addEventListener('loadedmetadata', () => {
      timeEl.textContent = `${fmtMapTime(aud.currentTime)} / ${fmtMapTime(aud.duration)}`;
    });
    aud.addEventListener('timeupdate', () => {
      if (aud.duration) seekEl.value = (aud.currentTime / aud.duration) * 100;
      timeEl.textContent = `${fmtMapTime(aud.currentTime)} / ${fmtMapTime(aud.duration)}`;
    });
    seekEl.oninput = () => {
      if (aud.duration) aud.currentTime = (Number(seekEl.value) / 100) * aud.duration;
    };
    volEl.oninput = () => {
      aud.volume = Number(volEl.value);
    };
  }
})();

(() => {
  const cancelBtn = $('sepCancelBtn');
  if (cancelBtn) {
    cancelBtn.onclick = () => $('cancelJobBtn')?.click();
  }
  setInterval(() => {
    const prog = $('separationProgress'), pct = $('sepPercentText');
    if (prog && pct) {
      pct.textContent = Math.round(Number(prog.value) || 0) + '%';
    }
  }, 300);
})();

(() => {
  const PLAY_SVG = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 2.8a.6.6 0 0 1 .9-.5l8 5.2a.6.6 0 0 1 0 1l-8 5.2a.6.6 0 0 1-.9-.5V2.8Z"/></svg>';
  const PAUSE_SVG = '<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="3.5" y="2.8" width="3" height="10.4" rx=".8"/><rect x="9.5" y="2.8" width="3" height="10.4" rx=".8"/></svg>';

  function syncPlaySvg(btn, isPaused) {
    if (!btn) return;
    const stateKey = isPaused ? 'play' : 'pause';
    if (btn.dataset.svgState === stateKey && btn.firstElementChild?.tagName === 'svg') return;
    btn.dataset.svgState = stateKey;
    btn.innerHTML = isPaused ? PLAY_SVG : PAUSE_SVG;
  }

  const mainBtn = $('playBtn');
  if (mainBtn && typeof audio !== 'undefined') {
    syncPlaySvg(mainBtn, audio.paused);
    for (const ev of ['play', 'pause', 'ended']) {
      audio.addEventListener(ev, () => queueMicrotask(() => syncPlaySvg(mainBtn, audio.paused)));
    }
  }

  for (const wrapEl of document.querySelectorAll('.modern-audio-player[data-for-audio]')) {
    const aud = $(wrapEl.dataset.forAudio);
    const btn = wrapEl.querySelector('.map-play');
    if (aud && btn) {
      syncPlaySvg(btn, aud.paused);
      for (const ev of ['play', 'pause', 'ended']) {
        aud.addEventListener(ev, () => queueMicrotask(() => syncPlaySvg(btn, aud.paused)));
      }
    }
  }
})();
