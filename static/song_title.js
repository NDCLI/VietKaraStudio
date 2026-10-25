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
  root.innerHTML=(box?`<div style="${boxStyle}"></div>`:'')+rows.map(row=>`<div style="position:absolute;white-space:nowrap;left:${row.x*scale}px;top:${row.y*scale}px;font-size:${row.size*scale}px;font-family:${esc(plan.font)};font-weight:${plan.weight};font-style:${plan.italic?'italic':'normal'};line-height:1.2;color:${plan.color};-webkit-text-stroke:${plan.outline_width*scale}px ${plan.outline};paint-order:stroke fill;text-shadow:${plan.shadow?`${plan.shadow*scale}px ${plan.shadow*scale}px ${plan.shadow*1.5*scale}px ${plan.shadow_color}`:'none'};opacity:${opacity};transform:translate(${m.x*row.size*.65*scale}px,${m.y*row.size*.65*scale}px) scale(${m.scale}) rotateX(${m.rx}deg);transform-origin:left top">${esc(row.text)}</div>`).join('');
}

function renderWaveTrackControls(){
  const root=$('waveTrackControls');if(!root||!state.project)return;
  const enabled=slot=>slot==='vocals'?state.project.vocals_ready:!!state.project[slot];
  const icon=active=>`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M3 9h4l5-4v14l-5-4H3Z"/>${active?'<path d="M16 8a6 6 0 0 1 0 8M19 5a10 10 0 0 1 0 14"/>':'<path d="m16 9 6 6m0-6-6 6"/>'}</svg>`;
  root.innerHTML=[['beat','Beat'],['reference','Có vocal'],['vocals','Vocal riêng']].map(([slot,label])=>{
    const active=state.track===slot&&!audio.muted;
    return `<button data-monitor-track="${slot}" aria-label="${active?'Tắt tiếng':'Nghe'} ${label}" aria-pressed="${active}" title="${enabled(slot)?'Bấm để nghe hoặc tắt tiếng':'Chưa có '+label}" ${!enabled(slot)||state.manualTiming?'disabled':''}>${icon(active)}<span>${label}</span></button>`;
  }).join('');
}
function initWaveTrackControls(){
  $('waveTrackControls').onclick=wrap(async event=>{
    const button=event.target.closest('[data-monitor-track]');if(!button||state.manualTiming)return;
    const track=button.dataset.monitorTrack;
    if(state.track===track)audio.muted=!audio.muted;
    else{
      const playing=!audio.paused,position=beatNow();audio.muted=false;setAudio(track,position,playing);
    }
    renderWaveTrackControls();drawTimeline();
  });
}
