'use strict';
function initSongTitle(){
  const choices={none:'Không',fade:'Mờ dần',slide_up:'Trượt lên',slide_down:'Trượt xuống',slide_left:'Trượt trái',slide_right:'Trượt phải',zoom:'Phóng to / thu nhỏ',flip:'Lật chữ'};
  for(const id of ['titleEnter','titleExit'])$(id).innerHTML=Object.entries(choices).map(([key,label])=>`<option value="${key}">${label}</option>`).join('');
  const templatePresets={gradient:'vcl-pastel_mem',midnight:'vcl-vang_champagne',crystal:'vcl-chrome_bac',neon:'vcl-neon_vien_manh',retro:'vcl-retro_vien_doi',pastel:'vcl-hong_ngoc_rose'};
  document.querySelector('.title-template-strip').onclick=wrap(async event=>{
    const button=event.target.closest('[data-title-template]');if(!button||state.job)return;
    const template=button.dataset.titleTemplate;
    if(!state.presets.length)await loadPresets();
    const preset=state.presets.find(p=>p.id===templatePresets[template]),style=preset?.settings||{};
    Object.assign(state.project.settings,{title_template:template,title_enabled:true,title_text:state.project.settings.title_text||state.project.name,
      title_font:style.font||'Arial',title_color:style.sung||'#ffffff',title_outline:style.outline_sung||'#15202a',
      title_outline_width:style.outline_width??3,title_shadow:style.shadow??2,title_shadow_color:style.shadow_color||'#000000',
      title_weight:template==='neon'||template==='retro'?800:700,title_italic:!!style.italic,
      title_enter:template==='gradient'?'slide_up':template==='crystal'?'fade':'fade',title_exit:'fade'});
    renderStyle();dirty();
  });
  $('previewTitleEffect').onclick=wrap(()=>{
    if(state.job)return;
    const s=state.project.settings;
    if(s.title_end<=s.title_start)throw Error('Thời gian kết thúc tiêu đề phải sau bắt đầu.');
    if(state.project.beat&&s.title_start>=state.project.beat.duration)throw Error('Tiêu đề phải bắt đầu trong thời lượng Beat.');
    s.title_enabled=true;s.title_text=s.title_text||state.project.name;
    audio.pause();state.titlePreviewAt=performance.now();renderStyle();dirty();
  });
}
function drawSongTitle(t){
  let root=$('songTitlePreview');
  if(!root){root=document.createElement('div');root.id='songTitlePreview';$('preview').append(root);}
  const plan=state.layout?.title;
  if(!state.project.settings.title_enabled)state.titlePreviewAt=null;
  if(state.titlePreviewAt!=null&&plan){t=plan.start+(performance.now()-state.titlePreviewAt)/1000;if(t>plan.end){state.titlePreviewAt=null;}}
  root.hidden=!plan||t<plan.start||t>=plan.end;
  if(root.hidden)return;
  const rows=plan.preview_rows||plan.rows,box=plan.preview_box||plan.box;
  const scale=$('preview').clientWidth/state.layout.width;
  const duration=plan.effect_seconds,enter=Math.min(1,(t-plan.start)/duration),exit=Math.min(1,(plan.end-t)/duration);
  function transform(mode,phase,leaving){
    const p=1-phase,sign=leaving?-1:1;
    const positions={slide_up:[0,1],slide_down:[0,-1],slide_left:[1,0],slide_right:[-1,0]},offset=positions[mode]||[0,0];
    return {x:offset[0]*p*sign,y:offset[1]*p*sign,scale:mode==='zoom'?1-.35*p:1,rx:mode==='flip'?75*p:0};
  }
  const arriving=enter<1,mode=arriving?plan.enter:plan.exit,phase=arriving?enter:exit,m=transform(mode,phase,!arriving);
  const opacity=mode==='none'?1:phase;
  const boxStyle=box?`position:absolute;left:${box.x*scale}px;top:${box.y*scale}px;width:${box.width*scale}px;height:${box.height*scale}px;background:linear-gradient(110deg,${box.colors.join(',')});opacity:${opacity*box.opacity/100};border:${box.border_width*scale}px solid ${box.border};border-radius:${box.radius*scale}px;transform:translate(${m.x*rows[0].size*.65*scale}px,${m.y*rows[0].size*.65*scale}px) scale(${m.scale}) rotateX(${m.rx}deg);transform-origin:left top;` : '';
  root.innerHTML=(box?`<div style="${boxStyle}"></div>`:'')+rows.map(row=>`<div style="position:absolute;white-space:nowrap;left:${row.x*scale}px;top:${row.y*scale}px;font-size:${row.size*scale}px;font-family:${esc(plan.font)};font-weight:${plan.weight};font-style:${plan.italic?'italic':'normal'};line-height:1.2;color:${plan.color};-webkit-text-stroke:${plan.outline_width*scale}px ${plan.outline};paint-order:stroke fill;text-shadow:${plan.shadow?`${plan.shadow*scale}px ${plan.shadow*scale}px ${plan.shadow*1.5*scale}px ${plan.shadow_color}`:'none'};opacity:${opacity};transform:translate(${m.x*row.size*.65*scale}px,${m.y*row.size*.65*scale}px) scale(${m.scale}) rotateX(${m.rx}deg);transform-origin:left top">${esc(row.display_text??row.text)}</div>`).join('');
}

const auxAudios={};
function trackEnabled(slot){
  if(!state.project)return false;
  return slot==='vocals'?!!state.project.vocals_ready:slot==='backing'?!!state.project.backing_ready:!!state.project[slot];
}
function ensureTrackState(){
  state.trackVolumes=state.trackVolumes||{beat:1,reference:1,vocals:1,backing:1};
  if(!state.trackMuted)state.trackMuted={beat:false,reference:true,vocals:true,backing:false};
  state.hiddenWaveLanes=state.hiddenWaveLanes||new Set();
}
function syncAuxTracks(){
  if(!state.project)return;
  ensureTrackState();
  const vocSlot=state.project.vocals_ready?'vocals':'reference';
  const slots=['beat',vocSlot,'backing'];
  // Ensure master audio is on an enabled track (prefer beat)
  const masterSlot=trackEnabled(state.track)?state.track:(slots.find(trackEnabled)||'beat');
  audio.muted=!!state.trackMuted[masterSlot]||!trackEnabled(masterSlot);
  audio.volume=state.trackVolumes[masterSlot]??1;
  const masterBeatTime=beatNow();
  for(const slot of slots){
    if(slot===masterSlot||!trackEnabled(slot)||state.trackMuted[slot]){
      if(auxAudios[slot]&&!auxAudios[slot].paused)auxAudios[slot].pause();
      continue;
    }
    let el=auxAudios[slot];
    if(!el){el=new Audio();el.preload='auto';auxAudios[slot]=el;}
    const st=Math.round(Number(state.project?.settings?.pitch_semitones||0));
    const wantSrc=location.origin+endpoint(`/audio/${slot}`)+`?v=${state.project.updated_at}`+(st?`&pitch=${st}`:'');
    if(el.src!==wantSrc)el.src=wantSrc;
    el.muted=false;
    el.volume=state.trackVolumes[slot]??1;
    el.preservesPitch=audio.preservesPitch;
    el.playbackRate=audio.playbackRate;
    const targetTime=slot==='beat'||slot==='backing'?masterBeatTime:mapTime(masterBeatTime,true);
    if(Number.isFinite(targetTime)&&Math.abs((el.currentTime||0)-targetTime)>0.08){
      try{el.currentTime=Math.max(0,targetTime);}catch{}
    }
    if(!audio.paused&&el.paused)el.play().catch(()=>{});
    else if(audio.paused&&!el.paused)el.pause();
  }
}
function renderWaveTrackControls(){
  const root=$('waveTrackControls');if(!root||!state.project)return;
  ensureTrackState();
  const spk=active=>`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M3 9h4l5-4v14l-5-4H3Z"/>${active?'<path d="M16 8a6 6 0 0 1 0 8M19 5a10 10 0 0 1 0 14"/>':'<path d="m16 9 6 6m0-6-6 6"/>'}</svg>`;
  const eye=off=>off?`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M3 3l18 18M10.6 10.6a3 3 0 0 0 4.2 4.2M9.9 5.1A10.7 10.7 0 0 1 12 5c6.5 0 10 7 10 7a18.5 18.5 0 0 1-3.2 4.2M6.2 6.2C3.6 8.1 2 12 2 12s3.5 7 10 7a10.6 10.6 0 0 0 4.8-1.1"/></svg>`:`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6Z"/><circle cx="12" cy="12" r="3"/></svg>`;
  const vocSlot=state.project.vocals_ready?'vocals':'reference';
  const rows=[['beat','NHẠC NỀN'],[vocSlot,'GIỌNG HÁT'],['backing','GIỌNG BÈ']];
  const unmutedSlots=rows.map(r=>r[0]).filter(s=>trackEnabled(s)&&!state.trackMuted[s]);
  root.innerHTML=rows.map(([resolved,label])=>{
    const laneKey=resolved==='vocals'?'reference':resolved;
    const isEnabled=trackEnabled(resolved);
    const active=isEnabled&&!state.trackMuted[resolved];
    const isSolo=active&&unmutedSlots.length===1&&unmutedSlots[0]===resolved;
    const vol=state.trackVolumes[resolved]??1;
    const hiddenLane=state.hiddenWaveLanes.has(laneKey);
    const dis=!isEnabled||state.manualTiming?'disabled':'';
    return `<div class="wtc-lane ${active?'on':''}">
      <div class="wtc-head"><strong>${label}</strong><button type="button" class="wtc-eye ${hiddenLane?'off':''}" data-hide-lane="${laneKey}" title="${hiddenLane?'Hiện':'Ẩn'} sóng ${label}" aria-label="${hiddenLane?'Hiện':'Ẩn'} sóng ${label}">${eye(hiddenLane)}</button></div>
      <div class="wtc-controls">
        <button type="button" class="wtc-solo ${isSolo?'on':''}" data-monitor-track="${resolved}" aria-pressed="${isSolo}" title="Solo chỉ nghe ${label}" ${dis}>S</button>
        <button type="button" class="wtc-mute ${active?'on':''}" data-mute-track="${resolved}" aria-pressed="${active}" title="${active?'Tắt tiếng':'Bật nghe thêm'} ${label}" ${dis}>${spk(active)}</button>
        <input type="range" class="wtc-vol" data-vol-track="${resolved}" min="0" max="1" step="0.02" value="${vol}" title="Âm lượng ${label}" ${dis}>
      </div>
    </div>`;
  }).join('');
  syncAuxTracks();
}
function initWaveTrackControls(){
  for(const ev of ['play','pause','seeking','seeked','ratechange','timeupdate']){
    audio.addEventListener(ev,syncAuxTracks);
  }
  $('waveTrackControls').onclick=wrap(async event=>{
    ensureTrackState();
    const eyeBtn=event.target.closest('[data-hide-lane]');
    if(eyeBtn){
      const lane=eyeBtn.dataset.hideLane;
      if(state.hiddenWaveLanes.has(lane))state.hiddenWaveLanes.delete(lane);
      else state.hiddenWaveLanes.add(lane);
      renderWaveTrackControls();drawTimeline();return;
    }
    const solo=event.target.closest('[data-monitor-track]');
    if(solo&&!state.manualTiming){
      const track=solo.dataset.monitorTrack;
      const vocSlot=state.project.vocals_ready?'vocals':'reference';
      const allSlots=['beat',vocSlot,'backing'].filter(trackEnabled);
      const unmuted=allSlots.filter(s=>!state.trackMuted[s]);
      if(unmuted.length===1&&unmuted[0]===track){
        for(const s of allSlots)state.trackMuted[s]=false;
      }else{
        for(const s of allSlots)state.trackMuted[s]=(s!==track);
      }
      if(!audio.getAttribute('src')&&trackEnabled(track))setAudio(track,beatNow(),!audio.paused);
      renderWaveTrackControls();drawTimeline();return;
    }
    const mute=event.target.closest('[data-mute-track]');
    if(mute&&!state.manualTiming){
      const track=mute.dataset.muteTrack;
      state.trackMuted[track]=!state.trackMuted[track];
      if(!state.trackMuted[track]&&!audio.getAttribute('src')&&trackEnabled(track)){
        setAudio(track,beatNow(),!audio.paused);
      }
      renderWaveTrackControls();drawTimeline();
    }
  });
  $('waveTrackControls').oninput=event=>{
    const slider=event.target.closest('[data-vol-track]');if(!slider)return;
    ensureTrackState();
    const track=slider.dataset.volTrack,v=Number(slider.value);
    state.trackVolumes[track]=v;
    if(v>0&&state.trackMuted[track])state.trackMuted[track]=false;
    syncAuxTracks();
  };
  $('waveTrackControls').onchange=event=>{
    if(!event.target.closest('[data-vol-track]'))return;
    renderWaveTrackControls();
  };
}
