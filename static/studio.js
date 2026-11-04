'use strict';
// Geometry comes from the same measured plan used by the ASS exporter.
let layoutRevision=0,layoutTimer=null;
function scheduleLayout(){
  const revision=++layoutRevision;clearTimeout(layoutTimer);
  layoutTimer=setTimeout(async()=>{
    const p=state.project;if(!p)return;
    try{
      const result=await api('/api/layout',{method:'POST',body:{project_id:p.id,settings:p.settings,words:p.words,anchors:p.anchors,scenes:p.scenes||[],beat:p.beat?{duration:p.beat.duration}:null}});
      if(revision!==layoutRevision||state.project?.id!==p.id)return;
      state.layout=result;
      $('fontNote').hidden=!result.font_fallback;
      $('fontNote').textContent=result.font_fallback?'Không tìm thấy font đã chọn; đang dùng '+result.font+'.':'';
      const conflicts=(result.warnings||[]).filter(w=>w.type==='overlap');
      $('layoutNotice').hidden=!conflicts.length;
      $('layoutNotice').textContent=conflicts.length?`${conflicts.length} cặp câu trùng mốc trên cùng hàng. Chọn câu để đổi hàng hoặc sửa thời gian.`:'';
      studioFrame();drawTimeline();
    }catch(error){if(revision===layoutRevision)handleError(error);}
  },75);
}
function studioStyle(){
  const p=state.project;if(!p)return;const s=p.settings,preview=$('preview');s.display_mode='karaoke';
  renderSourceAssets();
  $('fontSample').style.fontFamily=s.font;$('fontSample').style.fontStyle=s.italic?'italic':'normal';
  preview.style.backgroundColor=s.background;preview.classList.toggle('portrait',s.aspect==='portrait');
  preview.style.backgroundImage='';
  let videoSlot=null,info='';
  if(s.background_type==='original'){
    videoSlot=p.reference?.video?'reference':p.beat?.video?'beat':null;
    info=videoSlot?'Video gốc: '+p[videoSlot].name:'Chưa có video gốc. Nạp bản hát/beat dạng video để dùng.';
  }else if(s.background_type==='video'){
    videoSlot=p.background_video?'background':null;info=p.background_video?.name||'Chưa nạp video nền.';
  }else if(s.background_type==='image'||(s.background_type==='auto'&&p.background_name&&s.use_background)){
    if(p.background_name)preview.style.backgroundImage=`url('${endpoint('/background')}?v=${p.updated_at}')`;
    info=p.background_name||'Chưa nạp ảnh nền.';
  }else info='Màu nền '+s.background;
  $('backgroundFileTools').hidden=['original','color'].includes(s.background_type);
  const video=$('backgroundVideo'),url=videoSlot?endpoint('/video/'+videoSlot)+'?v='+p.updated_at:'';
  if(video.getAttribute('data-source')!==url){video.pause();video.setAttribute('data-source',url);if(url)video.src=url;else{video.removeAttribute('src');video.load();}}
  video.hidden=!url;$('backgroundInfo').textContent=info;
  $('resolutionBadge').textContent=s.resolution==='4k'?'4K':'1080P';
  $('renderBtn').textContent='↗ Dựng video '+(s.render_format||'mp4').toUpperCase()+' · '+(s.resolution==='4k'?'4K':'1080p');
  document.querySelectorAll('[data-display-mode]').forEach(b=>{const on=b.dataset.displayMode===s.display_mode;b.classList.toggle('on',on);b.setAttribute('aria-pressed',String(on));});
  document.querySelectorAll('[data-singing-mode]').forEach(b=>{const on=b.dataset.singingMode===s.singing_mode;b.classList.toggle('on',on);b.setAttribute('aria-pressed',String(on));});
  document.querySelectorAll('[data-title-template]').forEach(b=>{const on=b.dataset.titleTemplate===s.title_template;b.classList.toggle('on',on);b.setAttribute('aria-pressed',String(on));});
  document.querySelectorAll('[data-align]').forEach(b=>{const on=b.dataset.align===s.align;b.classList.toggle('on',on);b.setAttribute('aria-pressed',String(on));});
  document.querySelectorAll('[data-karaoke-only]').forEach(el=>el.hidden=s.display_mode!=='karaoke');
  $('modeHint').textContent={karaoke:'Hai hàng cố định, quét màu theo từng từ.',lyricsmv:'Lời nổi bật theo từng câu trên nền nhạc/video.',subtitle:'Phụ đề hiện trọn câu theo mốc đã căn.'}[s.display_mode];
  $('roleAnalysisStatus').textContent=p.role_analysis?roleAnalysisMessage(p.role_analysis):'Dùng vocal đã tách; chưa có vocal thì app tự tách trước.';
  $('duetSettings').hidden=s.singing_mode!=='duet'||s.display_mode!=='karaoke';
  $('mvSettings').hidden=s.display_mode!=='lyricsmv';$('subtitleSettings').hidden=s.display_mode!=='subtitle';
  $('countdownUpload').hidden=s.countdown!=='image'||s.display_mode!=='karaoke';
  $('gradientSettings').hidden=!s.sung_gradient;
  $('wordRole').querySelector('[value=male]').textContent=s.role_a_name;
  $('wordRole').querySelector('[value=female]').textContent=s.role_b_name;
  for(const [role,key] of [['voice_c','role_c_name'],['voice_d','role_d_name'],['voice_e','role_e_name'],['voice_f','role_f_name'],['unknown','role_unknown_name']])$('wordRole').querySelector(`[value=${role}]`).textContent=s[key];
  $('logoInfo').textContent=p.logo_name||'PNG nền trong suốt sẽ giữ nguyên độ trong suốt.';
  $('sourceVideoInfo').textContent=p.background_video?.name||p.background_name||'';
  $('sourceVideoTools').hidden=!p.background_video;
  $('sourceVideoActions').hidden=!p.background_video;
  renderStyleControls();
  renderSceneControls();
  document.querySelectorAll('[data-value-for]').forEach(el=>el.textContent=$(el.dataset.valueFor).value);
  fitStudioPreview();scheduleLayout();studioFrame();
}
function rowMotion(row,s,t){
  const duration=row.show_end-row.show_start,ms=Math.min(.6,duration/3),age=t-row.show_start,remaining=row.show_end-t;
  const enter=Math.max(0,Math.min(1,age/Math.max(.001,ms))),exit=Math.max(0,Math.min(1,remaining/Math.max(.001,ms)));
  let opacity=1,scale=1,dx=0,dy=0,rx=0;
  const mode=s.line_animation;
  if(mode!=='none')opacity=(mode==='fadesec'&&!row.section_start?1:enter)*(mode==='fadesec'&&!row.section_end?1:exit);
  const phase=1-Math.min(enter,exit);
  if(mode==='zoom')scale=1-.35*phase;
  if(mode==='zoomout'||mode==='punchzoom')scale=1+.35*phase;
  if(mode==='popbounce')scale=enter<.5?.7+.84*enter:1+.12*(1-enter)*2;
  if(mode==='flipup')rx=70*phase;
  if(mode==='split')dx=row.font_size*(row.align==='right'?-1:1)*(1-enter);
  if(mode==='slidedown')dy=-row.font_size*(1-enter);
  if(mode==='slideup')dy=row.font_size*(1-enter);
  return {opacity,scale,dx,dy,rx};
}
function roleIconMarkup(row,settings,scale){
  const marker=row.role_icon;if(!marker)return '';
  return `<img class="studio-role-icon" src="${esc(marker.url)}" alt="${esc(marker.label)}" style="left:${(marker.x-row.x)*scale}px;top:${(marker.y-row.y)*scale}px;width:${marker.width*scale}px;height:${marker.size*scale}px;object-fit:contain">`;
}
function studioFrame(){
  if(!state.project)return;
  const p=state.project,s=p.settings,t=beatNow(),plan=state.manualTiming?.plan||state.layout,stage=$('preview'),lyrics=$('previewLyrics');
  $('previewEmpty').hidden=!!p.words.length;
  $('previewTime').textContent=time(t);$('currentTime').textContent=time(audio.currentTime,true);if(document.activeElement!==$('seek'))$('seek').value=audio.currentTime;
  syncBackground(t);syncSceneImages(t,state.layout);updateLogoFrame(t);drawSongTitle(t);
  if(!plan){lyrics.innerHTML='';$('previewCountdown').hidden=true;return;}
  const scale=stage.clientWidth/plan.width;
  let rows=[];
  if(s.display_mode==='karaoke'&&state.manualTiming){
    rows=tappingRows(plan);
  }else if(s.display_mode==='karaoke'){
    for(let slot=0;slot<2;slot++){
      const candidates=plan.rows.filter(r=>r.slot===slot&&t>=r.show_start&&t<=r.show_end);
      const singing=candidates.filter(r=>t>=r.start&&t<=r.end);
      const chosen=(singing.length?singing:candidates).sort((a,b)=>b.start-a.start)[0];
      if(chosen)rows.push({...chosen,opacity:1});
    }
  }else{
    const active=plan.rows.find(r=>t>=r.show_start&&t<=r.show_end);
    if(active){
      rows=[{...active,opacity:1}];
      if(s.display_mode==='lyricsmv'&&s.mv_style!=='single'){
        const count=s.mv_style==='scroll'?2:1;
        for(let d=-count;d<=count;d++){
          const neighbor=plan.rows[active.index+d];if(!d||!neighbor)continue;
          const w=neighbor.width*.7;
          const x=active.align==='center'?(plan.width-w)/2:active.align==='left'?active.x:active.x+active.width-w;
          rows.push({...active,text:neighbor.text,display_text:neighbor.display_text,x,y:active.y+d*active.font_size*s.line_height,width:w,font_size:neighbor.font_size*.7,opacity:Math.max(.2,.65-Math.abs(d)*.15),ghost:true});
        }
      }
    }
  }
  lyrics.style.fontFamily=plan.font;lyrics.style.fontStyle=s.italic?'italic':'normal';
  const markup=rows.map(row=>{
    const motion=state.manualTiming?{opacity:1,dx:0,dy:0,scale:1,rx:0}:rowMotion(row,s,t),px=v=>(v*scale).toFixed(3)+'px';
    const fontSize=row.font_size*plan.font_factor*scale;
    const shadow=s.shadow?`${s.shadow*scale}px ${s.shadow*scale}px ${s.shadow*scale}px ${s.shadow_color}`:'none';
    const rowStyle=`left:${px(row.x)};top:${px(row.y)};font-size:${fontSize}px;line-height:1.2;opacity:${motion.opacity*row.opacity};transform:translate(${px(motion.dx)},${px(motion.dy)}) scale(${motion.scale}) rotateX(${motion.rx}deg);transform-origin:${row.align==='right'?'right':row.align==='center'?'center':'left'} bottom;--outline-width:${px(s.outline_width*2)};--outline-u:${colorAlpha(s.outline_unsung,s.outline_unsung_opacity)};--outline-s:${colorAlpha(s.outline_sung,s.outline_sung_opacity)};--text-shadow:${shadow};`;
    if(s.display_mode!=='karaoke'){
      const box=s.display_mode==='subtitle'&&s.subtitle_box?`background:${colorAlpha(s.subtitle_box_color,s.subtitle_box_opacity)};border-radius:${px(s.subtitle_box_radius)};padding:${px(row.font_size*.18)};margin:-${px(row.font_size*.18)};`:'';
      const color=s.display_mode==='subtitle'||row.ghost?s.unsung:row.color;
      const gradient=s.sung_gradient&&!row.ghost?`background-image:linear-gradient(to top,${s.gradient_bottom} ${s.gradient_start}%,${s.gradient_top} ${s.gradient_end}%);background-clip:text;-webkit-background-clip:text;color:transparent;`:'';
      return `<div class="studio-line plain-line" style="${rowStyle}${box}"><span style="color:${color};${gradient}display:inline-block;transform:scaleX(${s.horizontal_scale/100});transform-origin:left top">${esc(row.display_text??row.text)}</span></div>`;
    }
    const words=row.words.map(w=>{
      const source=p.words[w.source_index],word=source?wordOnBeat(source):w;
      const normal=Math.max(0,Math.min(1,(t-word.start)/Math.max(.01,word.end-word.start)));
      const progress=tappingProgress(w.source_index,t,word,normal);
      const size=w.width*scale/(s.horizontal_scale/100);
      // Include italic overhang and outline in the mask surface.
      const pad=fontSize*.4+s.outline_width*scale,edge=(pad+progress*size)/(size+2*pad)*100;
      const feather=Math.min(35,Math.max(.8,fontSize*.025,s.sweep_feather/3*scale)/(size+2*pad)*100),angle=s.italic?101:90;
      const fillMask=progress<=0?'none':progress>=1?'none':`linear-gradient(${angle}deg,#000 ${edge-feather/2}%,transparent ${edge+feather/2}%)`;
      const baseMask='none'; // Keep the complete glyph beneath the colored sweep; no transparent seam.
      const grad=s.sung_gradient&&row.role==='all'?`background-image:linear-gradient(to top,${s.gradient_bottom} ${s.gradient_start}%,${s.gradient_top} ${s.gradient_end}%);background-clip:text;-webkit-background-clip:text;color:transparent;`:`color:${row.color};`;
      const glow=s.sweep_glow&&progress>0&&progress<1?`<span class="studio-glow" style="opacity:${Math.min(1,s.sweep_glow/150)};filter:blur(${(1+s.sweep_feather/12+s.sweep_glow/100)*scale}px);mask-image:linear-gradient(${angle}deg,transparent ${edge-10}%,#000 ${edge}%,transparent ${edge+10}%);-webkit-mask-image:linear-gradient(${angle}deg,transparent ${edge-10}%,#000 ${edge}%,transparent ${edge+10}%)">${esc(w.display_text??w.text)}</span>`:'';
      const shadowLayer=s.shadow?`<span class="studio-shadow" style="color:${s.shadow_color};-webkit-text-stroke:var(--outline-width) ${s.shadow_color};transform:translate(${px(s.shadow)},${px(s.shadow)})">${esc(w.display_text??w.text)}</span>`:'';
      return `<span class="studio-word" style="--mask-pad:${pad}px;left:${px(w.x-row.x)};width:${size.toFixed(3)}px;transform:scaleX(${s.horizontal_scale/100});transform-origin:left top">${shadowLayer}<span class="studio-base" style="color:${s.unsung};visibility:${progress>=1?'hidden':'visible'};mask-image:${baseMask};-webkit-mask-image:${baseMask}">${esc(w.display_text??w.text)}</span><span class="studio-fill" style="${grad}visibility:${progress<=0?'hidden':'visible'};mask-image:${fillMask};-webkit-mask-image:${fillMask}">${esc(w.display_text??w.text)}</span>${glow}</span>`;
    }).join('');
    return `<div class="studio-line karaoke-line" data-row="${row.slot}" style="${rowStyle}width:${px(row.width)};height:${px(row.height)}">${roleIconMarkup(row,s,scale)}${words}</div>`;
  }).join('');
  patchLyricPreview(lyrics,markup);
  const countdown=$('previewCountdown'),window=plan.countdowns.find(w=>t>=w.start&&t<w.end);
  countdown.hidden=!window||s.display_mode!=='karaoke'||s.countdown==='none';
  if(!countdown.hidden){
    const n=Math.max(0,Math.min(4,Math.ceil(window.end-t)));
    countdown.style.left=window.x*scale+'px';countdown.style.top=window.y*scale+'px';countdown.style.height=window.size*scale+'px';countdown.style.gap=window.size*.28*scale+'px';
    const shapes={mic:'M8 3Q8 0 12 0Q16 0 16 3V11Q16 16 12 16Q8 16 8 11ZM5 10H6Q6 19 12 19Q18 19 18 10H19Q19 19 13 20V23H18V25H6V23H11V20Q5 19 5 10Z',star:'M12 0L15 8 24 9 17 15 19 24 12 19 5 24 7 15 0 9 9 8Z',heart:'M12 23C-7 11 0-5 12 5C24-5 31 11 12 23Z',note:'M9 3L22 0V18C20 24 12 22 15 17Q17 14 20 15V6L11 9V21C8 27 0 24 3 20Q5 17 9 18Z',moon:'M20 1C1-4-7 22 13 25Q20 26 26 18C8 23 4 5 20 1Z'};
    const shape=s.countdown==='image'?`<img src="${endpoint('/countdown')}?v=${p.updated_at}" alt="">`:`<svg viewBox="-2 -2 30 30" aria-hidden="true"><path d="${shapes[s.countdown]||shapes.mic}" fill="${s.sung}" stroke="${s.outline_unsung}" stroke-width=".5"/></svg>`;
    countdown.innerHTML=Array.from({length:4},(_,j)=>`<span style="visibility:${j<n?'visible':'hidden'}">${shape}</span>`).join('');
  }
}
function syncBackground(t){
  const video=$('backgroundVideo');if(video.hidden||!video.getAttribute('src'))return;
  if(Number.isFinite(video.duration)&&video.duration>0){const target=Math.max(0,t)%video.duration;if(Math.abs(video.currentTime-target)>.2)video.currentTime=target;}
  video.playbackRate=audio.playbackRate;
  if(audio.paused){if(!video.paused)video.pause();}else if(video.paused)video.play().catch(()=>{});
}
function syncSceneImages(t,plan){
  const images=[$('sceneImageBack'),$('sceneImageFront')];
  if(!state.project||!plan?.scene_overlays?.length){for(const image of images)image.hidden=true;return;}
  const active=plan.scene_overlays.filter(scene=>t>=scene.start&&t<=scene.end).sort((a,b)=>a.start-b.start).slice(-2);
  for(let index=0;index<images.length;index++){
    const image=images[index],scene=active[index];
    if(!scene){image.hidden=true;continue;}
    if(image.getAttribute('data-source')!==scene.url){image.setAttribute('data-source',scene.url);image.src=scene.url;}
    image.hidden=false;
    let progress=Math.max(0,Math.min(1,(t-scene.start)/Math.max(.01,scene.end-scene.start)));
    const frames=scene.keyframes||[{},{x:0,y:0,scale:100,opacity:100}],from=frames[0]||{},to=frames[1]||from;
    const value=key=>Number(from[key]??(key==='scale'||key==='opacity'?100:0))+(Number(to[key]??(key==='scale'||key==='opacity'?100:0))-Number(from[key]??(key==='scale'||key==='opacity'?100:0)))*progress;
    const stage=$('preview'),scale=value('scale'),zoom=Math.max(1,scale/100),x=value('x')*stage.clientWidth/100*(zoom-1),y=value('y')*stage.clientHeight/100*(zoom-1);
    image.style.transformOrigin='center center';image.style.transform=`translate(${x}px,${y}px) scale(${scale/100})`;
    let opacity=scene.opacity/100*value('opacity')/100,fade=Math.max(0,scene.fade_seconds||0);
    if(fade&&t<scene.start+fade)opacity*=Math.max(0,Math.min(1,(t-scene.start)/fade));
    if(fade&&t>scene.end-fade)opacity*=Math.max(0,Math.min(1,(scene.end-t)/fade));
    image.style.opacity=String(opacity);
  }
}
function textFromWords(){
  const groups=new Map();for(const w of state.project.words){if(!groups.has(w.line))groups.set(w.line,[]);groups.get(w.line).push(w);}
  return [...groups.values()].map((row,i)=>(i&&row[0].section?'\n':'')+row.map(w=>w.text).join(' ')).join('\n');
}
function rebuildLineText(){
  const ids=new Map();for(const w of state.project.words){if(!ids.has(w.line))ids.set(w.line,ids.size);w.line=ids.get(w.line);}
  state.project.text=textFromWords();$('lyricsText').value=state.project.text;renderWords();renderSelected();drawTimeline();dirty();
}
function setStyleValue(key,value){if(!state.project||state.job)return;state.project.settings[key]=value;renderStyle();dirty();}
function initWorkspaceChrome(){
  const root=$('karaokePage'),saved=(()=>{try{return JSON.parse(localStorage.getItem('obs_studio_panels_v1')||'{}');}catch{return {};}})();
  root.classList.toggle('left-collapsed',!!saved.left);root.classList.toggle('right-collapsed',!!saved.right);
  root.classList.toggle('waves-hidden',!state.showWaves);
  $('collapseLeftBtn').textContent=saved.left?'›':'‹';$('collapseRightBtn').textContent=saved.right?'‹':'›';
  $('collapseLeftBtn').setAttribute('aria-label',saved.left?'Mở bảng lời và nguồn':'Thu gọn bảng lời và nguồn');$('collapseLeftBtn').title=saved.left?'Mở bảng':'Thu gọn bảng';
  $('collapseRightBtn').setAttribute('aria-label',saved.right?'Mở bảng thuộc tính':'Thu gọn bảng thuộc tính');$('collapseRightBtn').title=saved.right?'Mở bảng':'Thu gọn bảng';
  const savePanels=()=>{try{localStorage.setItem('obs_studio_panels_v1',JSON.stringify({left:root.classList.contains('left-collapsed'),right:root.classList.contains('right-collapsed')}));}catch{}};
  $('collapseLeftBtn').onclick=()=>{root.classList.toggle('left-collapsed');const collapsed=root.classList.contains('left-collapsed');$('collapseLeftBtn').setAttribute('aria-label',collapsed?'Mở bảng lời và nguồn':'Thu gọn bảng lời và nguồn');$('collapseLeftBtn').title=collapsed?'Mở bảng':'Thu gọn bảng';$('collapseLeftBtn').textContent=collapsed?'›':'‹';savePanels();fitStudioPreview();drawTimeline();};
  $('collapseRightBtn').onclick=()=>{root.classList.toggle('right-collapsed');const collapsed=root.classList.contains('right-collapsed');$('collapseRightBtn').setAttribute('aria-label',collapsed?'Mở bảng thuộc tính':'Thu gọn bảng thuộc tính');$('collapseRightBtn').title=collapsed?'Mở bảng':'Thu gọn bảng';$('collapseRightBtn').textContent=collapsed?'‹':'›';savePanels();fitStudioPreview();drawTimeline();};
  document.querySelectorAll('[data-left-tab]').forEach(button=>button.onclick=()=>{
    const name=button.dataset.leftTab;
    document.querySelectorAll('[data-left-tab]').forEach(tab=>{const active=tab===button;tab.classList.toggle('on',active);tab.setAttribute('aria-selected',String(active));});
    document.querySelectorAll('[data-left-panel]').forEach(panel=>panel.hidden=panel.dataset.leftPanel!==name);
  });
  $('editLyricsBtn').onclick=()=>{const editor=$('lyricsEditor');editor.open=!editor.open;$('editLyricsBtn').setAttribute('aria-expanded',String(editor.open));if(editor.open){document.querySelector('[data-left-tab="lyrics"]').click();$('lyricsText').focus();}};
  document.querySelectorAll('[data-menu-click]').forEach(button=>button.onclick=()=>{
    const target=button.dataset.menuClick;
    if(target==='timelineManualTab')showInspector('sync');
    else if(target==='resetLayoutBtn')resetStudioLayout();
    else $(target)?.click();
    button.closest('.menu-pop')?.removeAttribute('open');
  });
  document.querySelectorAll('.workspace-menu>.menu-pop').forEach(menu=>menu.addEventListener('toggle',()=>{
    if(menu.open)document.querySelectorAll('.workspace-menu>.menu-pop').forEach(other=>{if(other!==menu)other.open=false;});
  }));
}
const SCENE_SECTION_LABELS={intro:'Mở đầu',verse:'Verse','pre-chorus':'Pre-chorus',chorus:'Chorus',bridge:'Bridge',outro:'Outro',instrumental:'Nhạc dạo'};
function renderSceneControls(){
  const host=$('sceneList');if(!host||!state.project)return;
  const openMotion=new Set([...host.querySelectorAll('.scene-motion[open]')].map(details=>details.closest('[data-scene-card]')?.dataset.sceneCard).filter(Boolean));
  const sections=[...new Set(state.project.words.map(word=>word.section_name).filter(name=>SCENE_SECTION_LABELS[name]))];
  for(const scene of state.project.scenes||[])if(!sections.includes(scene.section_name))sections.push(scene.section_name);
  if(!sections.length){host.innerHTML='<p class="hint">Gán câu vào một phân đoạn trong danh sách lời để tạo cảnh riêng.</p>';return;}
  host.innerHTML=sections.map(section=>{
    const scene=(state.project.scenes||[]).find(item=>item.section_name===section),label=SCENE_SECTION_LABELS[section]||section;
    if(!scene)return `<div class="scene-card"><strong>${esc(label)}</strong><button type="button" class="secondary full small" data-create-scene="${esc(section)}">＋ Tạo cảnh nền</button></div>`;
    const transition=scene.transition==='cut'?'cut':`fade-${Number(scene.transition_seconds||.6)}`;
    const keyframes=scene.keyframes||[{},{x:0,y:0,scale:100,opacity:100}];
    const motion=keyframes.map((frame,index)=>`<fieldset><legend>${index?'Cuối':'Đầu'}</legend>${[['x','Dời ngang',-100,100],['y','Dời dọc',-100,100],['scale','Tỉ lệ',100,200],['opacity','Độ rõ',0,100]].map(([property,title,min,max])=>`<label>${title} · ${Number(frame[property]??(property==='scale'||property==='opacity'?100:0))}%<input type="range" min="${min}" max="${max}" step="1" value="${Number(frame[property]??(property==='scale'||property==='opacity'?100:0))}" data-scene-motion="${esc(scene.id)}" data-keyframe="${index}" data-motion-property="${property}"></label>`).join('')}</fieldset>`).join('');
    return `<div class="scene-card" data-scene-card="${esc(scene.id)}"><strong>${esc(label)}</strong>${scene.image_ready?'<p class="hint">Ảnh cảnh đã nạp · PNG nội bộ</p>':'<p class="hint">Chưa có ảnh. Đang dùng nền chung cho phân đoạn này.</p>'}<label class="background-upload"><input type="file" accept="image/png,image/jpeg,image/webp" hidden data-scene-file="${esc(scene.id)}">${scene.image_ready?'Đổi ảnh cảnh':'Chọn ảnh cảnh'}</label><div class="scene-card-actions"><label>Chuyển cảnh<select data-scene-transition="${esc(scene.id)}"><option value="cut"${transition==='cut'?' selected':''}>Cắt thẳng</option><option value="fade-0.6"${transition==='fade-0.6'?' selected':''}>Mờ · 0,6 giây</option><option value="fade-1.2"${transition==='fade-1.2'?' selected':''}>Mờ · 1,2 giây</option><option value="fade-2"${transition==='fade-2'?' selected':''}>Mờ · 2 giây</option></select></label><button type="button" class="ghost scene-remove" data-remove-scene="${esc(scene.id)}">Bỏ cảnh</button></div><label>Độ rõ ảnh · ${scene.opacity}%<input type="range" min="0" max="100" step="1" value="${scene.opacity}" data-scene-opacity="${esc(scene.id)}"></label><details class="scene-motion"><summary>Keyframe · vị trí, tỉ lệ, độ rõ</summary><div>${motion}</div><p class="hint">Thông số chuyển đều từ đầu đến cuối phân đoạn.</p></details></div>`;
  }).join('');
  for(const id of openMotion)host.querySelector(`[data-scene-card="${id}"] .scene-motion`)?.setAttribute('open','');
}
async function uploadSceneImage(id,file){
  if(!file)return;await saveNow();const body=new FormData();body.append('file',file);
  state.project=await api(endpoint(`/scenes/${id}/image`),{method:'POST',body});
  renderSceneControls();scheduleLayout();syncSceneImages(beatNow(),state.layout);toast('Đã cập nhật ảnh cảnh.');
}
function initSceneTools(){
  $('sceneList').onclick=wrap(async event=>{
    const create=event.target.closest('[data-create-scene]');
    if(create){remember();state.project.scenes=state.project.scenes||[];state.project.scenes.push({id:crypto.randomUUID().replaceAll('-',''),section_name:create.dataset.createScene,opacity:100,transition:'fade',transition_seconds:.6});await saveNow();state.project=await api(endpoint());renderSceneControls();scheduleLayout();return;}
    const remove=event.target.closest('[data-remove-scene]');
    if(remove){if(!confirm('Bỏ cảnh khỏi phân đoạn? Ảnh sẽ ngừng dùng trong bản xuất và vẫn được giữ để hoàn tác.'))return;remember();state.project=await api(endpoint(`/scenes/${remove.dataset.removeScene}`),{method:'DELETE'});renderSceneControls();scheduleLayout();updateHistoryButtons();toast('Đã bỏ cảnh khỏi phân đoạn.');}
  });
  $('sceneList').onchange=wrap(async event=>{
    const input=event.target.closest('[data-scene-file]');
    if(input){const file=input.files[0];await uploadSceneImage(input.dataset.sceneFile,file);input.value='';return;}
    const id=event.target.dataset.sceneTransition||event.target.dataset.sceneOpacity||event.target.dataset.sceneMotion;if(!id)return;
    const scene=(state.project.scenes||[]).find(item=>item.id===id);if(!scene)return;
    remember();
    if(event.target.matches('[data-scene-transition]')){const value=event.target.value;if(value==='cut'){scene.transition='cut';scene.transition_seconds=0;}else{scene.transition='fade';scene.transition_seconds=Number(value.slice(5));}}
    else if(event.target.matches('[data-scene-opacity]'))scene.opacity=Number(event.target.value);
    else{const index=Number(event.target.dataset.keyframe),property=event.target.dataset.motionProperty;scene.keyframes=scene.keyframes||[{x:0,y:0,scale:100,opacity:100},{x:0,y:0,scale:100,opacity:100}];scene.keyframes[index][property]=Number(event.target.value);}
    await saveNow();renderSceneControls();scheduleLayout();syncSceneImages(beatNow(),state.layout);updateHistoryButtons();
  });
}
function initStudio(){
  initWorkspaceChrome();
  initExportMenu();
  initBranding();
  initTimelineEditing();
  initLayoutResize();
  initWorkspaceTools();initSongTitle();initWaveTrackControls();
  initSceneTools();
  initTimelineTools();
  document.querySelectorAll('[data-inspector-tab]').forEach(b=>b.onclick=()=>showInspector(b.dataset.inspectorTab));
  $('toggleWavesBtn').onclick=()=>{state.showWaves=!state.showWaves;$('waveformPanel').hidden=!state.showWaves;$('waveformHeightGrip').hidden=!state.showWaves;$('karaokePage').classList.toggle('waves-hidden',!state.showWaves);$('toggleWavesBtn').setAttribute('aria-pressed',String(state.showWaves));drawTimeline();};
  document.querySelectorAll('[data-display-mode]').forEach(b=>b.onclick=()=>setStyleValue('display_mode',b.dataset.displayMode));
  document.querySelectorAll('[data-singing-mode]').forEach(b=>b.onclick=()=>setStyleValue('singing_mode',b.dataset.singingMode));
  document.querySelectorAll('[data-align]').forEach(b=>b.onclick=()=>setStyleValue('align',b.dataset.align));
  $('fontSmaller').onclick=()=>setStyleValue('font_size',Math.max(20,state.project.settings.font_size-2));
  $('fontLarger').onclick=()=>setStyleValue('font_size',Math.min(240,state.project.settings.font_size+2));
  $('countdownInput').onchange=wrap(async()=>{const input=$('countdownInput');await upload('countdown',input.files[0]);input.value='';});
  $('splitLineBtn').onclick=()=>{
    if(state.job)return;const w=selectedWord();if(!w)return;if(lineLocked(w.line))return toast('Mở khóa câu trước khi tách.');
    const words=state.project.words,first=words.findIndex(x=>x.line===w.line);
    if(first===state.selected)return toast('Từ này đã ở đầu câu.');
    remember();const old=w.line,next=Math.max(...words.map(x=>x.line))+1;
    words.forEach((x,i)=>{if(i>=state.selected&&x.line===old){x.line=next;delete x.row;delete x.section;}});rebuildLineText();
  };
  $('mergeLineBtn').onclick=()=>{
    if(state.job)return;const w=selectedWord();if(!w)return;if(lineLocked(w.line))return toast('Mở khóa câu trước khi gộp.');
    const groups=[...new Set(state.project.words.map(x=>x.line))],at=groups.indexOf(w.line);
    if(at<=0)return toast('Đây là câu đầu tiên.');
    const previous=state.project.words.find(x=>x.line===groups[at-1]);if(lineLocked(previous.line))return toast('Câu trước đang khóa. Mở khóa trước khi gộp.');
    remember();const old=w.line;
    for(const item of state.project.words)if(item.line===old){item.line=previous.line;item.role=previous.role;delete item.section;if(previous.row==null)delete item.row;else item.row=previous.row;}
    rebuildLineText();
  };
  $('autoRolesBtn').onclick=wrap(async()=>{
    if(state.job||state.manualTiming)return;
    if(!state.project.reference)throw Error('Cần bản hát có giọng để tự phân vai.');
    if(!state.project.words.length)throw Error('Nhận lời hoặc đồng bộ lời trước khi phân vai.');
    if(state.project.words.some(w=>w.role!=='all')&&!confirm('Tự phân vai sẽ thay vai đã gán ở các câu nhận được giọng. Giữ nguyên lời và thời gian. Tiếp tục?'))return;
    await saveNow();remember();await followJob(await api(endpoint('/suggest-roles'),{method:'POST',body:{device:'auto'}}));
  });
  $('swapRolesBtn').onclick=()=>{if(state.job||state.manualTiming)return;remember();for(const w of state.project.words)w.role=w.role==='male'?'female':w.role==='female'?'male':w.role;renderWords();renderSelected();renderStyle();drawTimeline();dirty();};
  $('alternateRolesBtn').onclick=()=>{if(state.job)return;remember();const ids=[...new Set(state.project.words.map(w=>w.line))];for(const w of state.project.words)w.role=ids.indexOf(w.line)%2?'female':'male';renderWords();drawTimeline();dirty();};
  initSourceAssets();
  showInspector('design');
}

function timelineGutter(width){return width<480?70:94;}
function showInspector(name){
  const designNames=['design','duet','title','background','scenes','logo','countdown','presets'];
  if(!designNames.includes(name)&&!['timing','sync'].includes(name))name='design';
  document.querySelectorAll('[data-inspector-tab]').forEach(button=>{const active=button.dataset.inspectorTab===name;button.classList.toggle('on',active);button.setAttribute('aria-selected',String(active));});
  const design=designNames.includes(name);document.querySelector('[data-inspector-pane="design"]').hidden=!design;$('wordTools').hidden=design;
  if(design){
    document.querySelectorAll('.style-panel [data-property-section]').forEach(section=>{section.hidden=section.dataset.propertySection!==name;section.open=!section.hidden;});
    $('inspectorScope').textContent=name==='scenes'?'Đang sửa: cảnh theo phân đoạn':'Đang sửa: cả bài';
    if(name==='scenes')renderSceneControls();
  }else{
    $('selectedPanel').hidden=name==='sync';$('syncPanel').hidden=name!=='sync';
    $('inspectorScope').textContent=name==='sync'?'Đang sửa: mốc đồng bộ':'Đang sửa: từ / câu';
  }
}
function compactProjectView(){
  const p=state.project;
  if(state.viewProject!==p.id){state.viewProject=p.id;showInspector('design');}
}
function studioTimeline(){
  const canvas=$('timeline'),rect=canvas.getBoundingClientRect();if(!rect.width||!state.project)return;
  const dpr=devicePixelRatio||1,W=rect.width,H=rect.height,G=timelineGutter(W),P=W-G;
  if(canvas.width!==Math.round(W*dpr)||canvas.height!==Math.round(H*dpr)){canvas.width=Math.round(W*dpr);canvas.height=Math.round(H*dpr);}
  const ctx=canvas.getContext('2d');ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,W,H);
  ctx.fillStyle='#0c1520';ctx.fillRect(0,0,W,H);
  if(state.followPlayhead&&!audio.paused&&!state.drag&&document.activeElement!==$('pan')){
    const duration=beatDuration(),span=Math.max(3,duration/state.zoom);
    state.pan=Math.max(0,Math.min(Math.max(0,duration-span),beatNow()-span*.35));
    $('pan').value=state.pan;
  }
  const range=timelineRange(),x=t=>G+(t-range.start)/range.span*P;
  const laneTop=29,gap=Math.max(40,(H-30)/2),blockH=Math.max(30,gap-8);
  ctx.fillStyle='#112a29';ctx.fillRect(G,laneTop-6,P,gap-3);ctx.fillStyle='#202d1b';ctx.fillRect(G,laneTop+gap-6,P,gap-3);
  ctx.save();ctx.beginPath();ctx.rect(G,0,P,H);ctx.clip();
  const step=[1,2,5,10,20,30,60,120,300,600].find(v=>v>=range.span/9)||600;
  ctx.font='9px Consolas';
  for(let t=Math.ceil(range.start/step)*step;t<=range.start+range.span;t+=step){const at=x(t);ctx.strokeStyle='#29404a';ctx.beginPath();ctx.moveTo(at,21);ctx.lineTo(at,H);ctx.stroke();ctx.fillStyle='#8aa1b5';ctx.fillText(time(t),at+3,14);}
  const slots=new Map();for(const row of state.layout?.rows||[])for(const w of row.words)slots.set(w.source_index,row.slot);
  state.rectangles=[];
  state.project.words.forEach((word,i)=>{
    const a=x(mapTime(word.start)),b=x(mapTime(word.end));if(b<G||a>W)return;
    const slot=slots.get(i)??word.row??word.line%2,y=laneTop+slot*gap,w=Math.max(3,b-a-1),selected=state.timelineSelection.has(i);
    const common=selectedWord()&&word.line===selectedWord().line;
    ctx.fillStyle=selected?'#48bfa6':word.estimated?'#34404a':slot?'#506d30':'#176a63';
    if(word.label_color)ctx.fillStyle=word.label_color;
    ctx.strokeStyle=selected?'#ffffff':common?'#b6ded4':word.estimated?'#9c935a':slot?'#85a650':'#309b8b';
    ctx.lineWidth=selected?1.7:1;ctx.beginPath();ctx.roundRect(a,y,w,blockH,4);ctx.fill();ctx.stroke();
    ctx.save();ctx.beginPath();ctx.rect(a+2,y,w-4,blockH);ctx.clip();ctx.font='10px Segoe UI';ctx.fillStyle=word.label_color?'#10151b':selected?'#082d23':'#e1ece5';ctx.fillText(word.text,a+5,y+blockH/2+4);ctx.restore();
    state.rectangles.push({i,x:a,y,w,h:blockH});
  });
  for(const anchor of state.project.anchors){const at=x(anchor.beat);ctx.strokeStyle='#dda85588';ctx.setLineDash([3,3]);ctx.beginPath();ctx.moveTo(at,22);ctx.lineTo(at,H);ctx.stroke();ctx.setLineDash([]);}
  const cursor=x(beatNow());if(cursor>=G&&cursor<=W){ctx.fillStyle='#ef8c76';ctx.fillRect(cursor,20,1,H);ctx.beginPath();ctx.moveTo(cursor,24);ctx.lineTo(cursor-4,17);ctx.lineTo(cursor+4,17);ctx.fill();}
  ctx.restore();ctx.fillStyle='#101c29';ctx.fillRect(0,0,G,H);
  ctx.font='bold 9px Segoe UI';
  ctx.fillStyle='#6cddcb';ctx.fillText('DÒNG TRÊN',9,laneTop+29);ctx.fillStyle='#b1d56f';ctx.fillText('DÒNG DƯỚI',9,laneTop+gap+29);
  ctx.strokeStyle='#455264';ctx.setLineDash([3,4]);ctx.beginPath();ctx.moveTo(G,laneTop+gap-8);ctx.lineTo(W,laneTop+gap-8);ctx.stroke();ctx.setLineDash([]);
}

// Keep glyph layers mounted while the mask moves; rebuilding the text each frame
// forces repeated shaping and paint and makes short word sweeps visibly uneven.
function patchLyricPreview(root,markup){
  const template=document.createElement('template');template.innerHTML=markup;
  function patch(a,b){
    if(a.childNodes.length!==b.childNodes.length){a.replaceChildren(...b.childNodes);return;}
    for(let i=0;i<a.childNodes.length;i++){
      const x=a.childNodes[i],y=b.childNodes[i];
      if(x.nodeType!==y.nodeType||x.nodeName!==y.nodeName){x.replaceWith(y);continue;}
      if(x.nodeType===3){if(x.nodeValue!==y.nodeValue)x.nodeValue=y.nodeValue;continue;}
      for(const attr of [...x.attributes])if(!y.hasAttribute(attr.name))x.removeAttribute(attr.name);
      for(const attr of y.attributes)if(x.getAttribute(attr.name)!==attr.value)x.setAttribute(attr.name,attr.value);
      patch(x,y);
    }
  }
  patch(root,template.content);
}

function initTimelineTools(){
  $('followTimelineBtn').onclick=()=>{
    state.followPlayhead=!state.followPlayhead;
    $('followTimelineBtn').setAttribute('aria-pressed',String(state.followPlayhead));
    $('followTimelineBtn').textContent='Theo nhạc · '+(state.followPlayhead?'bật':'tắt');
    $('followTimelineBtn').classList.toggle('on',state.followPlayhead);drawTimeline();
  };

// Anchor zoom at the time under the pointer, rather than jumping to the start.
for(const canvas of [$('timeline'),$('waveformCanvas')])canvas.addEventListener('wheel',event=>{
  if(!state.project||!event.deltaY)return;
  event.preventDefault();
  const rect=canvas.getBoundingClientRect(),gutter=timelineGutter(rect.width);
  const fraction=Math.max(0,Math.min(1,(event.clientX-rect.left-gutter)/Math.max(1,rect.width-gutter)));
  const old=timelineRange(),anchor=old.start+fraction*old.span;
  state.zoom=Math.max(1,Math.min(16,state.zoom*(event.deltaY<0?1.15:1/1.15)));
  $('zoom').step='.01';$('zoom').value=state.zoom;
  const span=Math.max(3,old.duration/state.zoom);
  state.pan=Math.max(0,Math.min(old.duration-span,anchor-fraction*span));
  $('pan').value=state.pan;drawTimeline();
},{passive:false});
$('timelineAutoBtn').onclick=()=>{if(state.job||state.manualTiming)return;if(state.project.reference&&state.project.beat)api(endpoint('/sync-suggest'),{method:'POST',body:{}}).then(followJob).catch(handleError);else $('alignLyricsBtn').click();};
$('timelineManualBtn').onclick=wrap(openTappingDialog);

}

function roleAnalysisMessage(result){
  if(result.two_registers)return `Gợi ý theo dải giọng: Nam ${result.male} · Nữ ${result.female} · Cùng hát ${result.shared} câu. ${result.uncertain?'Giữ nguyên '+result.uncertain+' câu chưa rõ giọng. ':''}Nghe lại và chỉnh vai nếu cần; cao độ không xác định chắc giới tính người hát.`;
  return (result.reason==='insufficient_voiced_audio'?'Chưa đủ đoạn vocal rõ để phân vai.':'Hai dải giọng còn chồng lấn, chưa đủ chắc để gán vai.')+' Giữ nguyên phân vai hiện tại; bạn có thể gán Nam / Nữ cho từng câu.';
}
