'use strict';
let studioWaveColors={};
const studioLayoutDefaults={left:280,right:320,waves:144,lyrics:140};
let studioLayoutSize={...studioLayoutDefaults};
function applyStudioLayout(){
  const root=$('karaokePage'),width=root.clientWidth;
  studioLayoutSize.left=Math.max(170,Math.min(studioLayoutSize.left,Math.max(170,width-500)));
  studioLayoutSize.right=Math.max(230,Math.min(studioLayoutSize.right,Math.max(230,width-studioLayoutSize.left-250)));
  studioLayoutSize.waves=Math.max(114,Math.min(studioLayoutSize.waves,Math.max(114,innerHeight*.24)));
  studioLayoutSize.lyrics=Math.max(124,Math.min(studioLayoutSize.lyrics,Math.max(124,innerHeight*.27)));
  for(const [key,value] of Object.entries(studioLayoutSize))root.style.setProperty('--layout-'+key,value+'px');
  for(const [id,value] of [['leftColumnGrip',studioLayoutSize.left],['rightColumnGrip',studioLayoutSize.right],['studioHeightGrip',studioLayoutSize.waves+studioLayoutSize.lyrics],['waveformHeightGrip',studioLayoutSize.waves]]){
    $(id).setAttribute('aria-valuemin','0');$(id).setAttribute('aria-valuemax',String(Math.max(innerWidth,innerHeight)));
    $(id).setAttribute('aria-valuenow',String(Math.round(value)));$(id).setAttribute('aria-valuetext',Math.round(value)+' px');
  }
  fitStudioPreview();scheduleTimelineDraw();
}
function saveStudioLayout(){try{localStorage.setItem('obs_studio_layout_v2',JSON.stringify(studioLayoutSize));}catch{}}
function resetStudioLayout(){
  studioLayoutSize={...studioLayoutDefaults};$('karaokePage').classList.remove('left-collapsed','right-collapsed','waves-hidden');
  state.showWaves=true;$('waveformPanel').hidden=false;$('waveformHeightGrip').hidden=false;$('toggleWavesBtn').setAttribute('aria-pressed','true');
  try{localStorage.removeItem('obs_studio_panels_v1');localStorage.removeItem('obs_studio_layout_v2');}catch{}
  $('collapseLeftBtn').textContent='‹';$('collapseRightBtn').textContent='›';
  $('collapseLeftBtn').setAttribute('aria-label','Thu gọn bảng lời và nguồn');$('collapseLeftBtn').title='Thu gọn bảng';
  $('collapseRightBtn').setAttribute('aria-label','Thu gọn bảng thuộc tính');$('collapseRightBtn').title='Thu gọn bảng';
  applyStudioLayout();saveStudioLayout();
}
function fitStudioPreview(){
  const column=document.querySelector('.editor-column'),stage=$('preview');
  if(!column||!stage||document.fullscreenElement===stage||!column.clientWidth||!column.clientHeight)return;
  const ratio=stage.classList.contains('portrait')?9/16:16/9;
  const width=Math.max(1,Math.min(column.clientWidth,Math.max(1,column.clientHeight-36)*ratio));
  stage.style.width=width.toFixed(2)+'px';stage.style.height=(width/ratio).toFixed(2)+'px';
}
function initLayoutResize(){
  const palette=getComputedStyle(document.documentElement);
  for(const key of ['beat','ref','backing','inactive'])studioWaveColors[key]=palette.getPropertyValue('--c-wave-'+key).trim();
  try{const saved=JSON.parse(localStorage.getItem('obs_studio_layout_v2'));for(const key of Object.keys(studioLayoutDefaults))if(Number.isFinite(saved?.[key]))studioLayoutSize[key]=saved[key];}catch{}
  const gripIds=['leftColumnGrip','rightColumnGrip','studioHeightGrip','waveformHeightGrip'];
  function update(id,dx,dy,initial){
    studioLayoutSize={...initial};
    if(id==='leftColumnGrip')studioLayoutSize.left+=dx;
    if(id==='rightColumnGrip')studioLayoutSize.right-=dx;
    if(id==='studioHeightGrip'){
      if(!state.showWaves)studioLayoutSize.lyrics=initial.lyrics-dy;
      else{
        const total=Math.max(190,initial.waves+initial.lyrics-dy),fraction=initial.waves/(initial.waves+initial.lyrics);
        studioLayoutSize.waves=total*fraction;studioLayoutSize.lyrics=total*(1-fraction);
      }
    }
    if(id==='waveformHeightGrip'){
      const change=Math.max(80-initial.waves,Math.min(initial.lyrics-110,dy));
      studioLayoutSize.waves+=change;studioLayoutSize.lyrics-=change;
    }
    applyStudioLayout();
  }
  for(const id of gripIds){
    const grip=$(id);let drag=null;
    grip.onpointerdown=event=>{if(event.button!==0)return;event.preventDefault();drag={x:event.clientX,y:event.clientY,initial:{...studioLayoutSize}};grip.setPointerCapture(event.pointerId);document.body.classList.add('resizing-layout');};
    grip.onpointermove=event=>{if(drag)update(id,event.clientX-drag.x,event.clientY-drag.y,drag.initial);};
    const finish=()=>{if(!drag)return;drag=null;document.body.classList.remove('resizing-layout');saveStudioLayout();};
    grip.onpointerup=finish;grip.onpointercancel=finish;
    grip.ondblclick=()=>{if(id==='leftColumnGrip')studioLayoutSize.left=studioLayoutDefaults.left;else if(id==='rightColumnGrip')studioLayoutSize.right=studioLayoutDefaults.right;else{studioLayoutSize.waves=studioLayoutDefaults.waves;studioLayoutSize.lyrics=studioLayoutDefaults.lyrics;}applyStudioLayout();saveStudioLayout();};
    grip.onkeydown=event=>{const delta={ArrowLeft:[-15,0],ArrowRight:[15,0],ArrowUp:[0,-15],ArrowDown:[0,15]}[event.key];if(!delta)return;event.preventDefault();update(id,...delta,{...studioLayoutSize});saveStudioLayout();};
  }
  const observer=new ResizeObserver(()=>{fitStudioPreview();scheduleTimelineDraw();});
  observer.observe(document.querySelector('.editor-column'));observer.observe($('waveformCanvas'));
  addEventListener('resize',applyStudioLayout);document.addEventListener('fullscreenchange',()=>{fitStudioPreview();studioFrame();});
  $('waveformCanvas').onpointerdown=event=>{
    if(state.manualTiming||!state.project)return;
    const rect=event.currentTarget.getBoundingClientRect(),g=timelineGutter(rect.width),range=timelineRange();
    seekBeat(Math.max(0,Math.min(range.duration,range.start+(event.clientX-rect.left-g)/(rect.width-g)*range.span)));scheduleTimelineDraw();
  };
  applyStudioLayout();
}
const studioWaveDetails=new Map();
function studioWaveDetail(slot,media,range,pixels){
  const pid=state.project.id,source=slot==='reference'&&state.project.vocals?'vocals':slot;
  const convert=t=>slot==='reference'?mapTime(t,true):t;
  const start=Math.max(0,Math.min(media.duration,convert(range.start))),end=Math.max(start,Math.min(media.duration,convert(range.start+range.span)));
  let entry=studioWaveDetails.get(slot);
  if(!entry||entry.pid!==pid||entry.media!==media){
    entry={pid,media,details:[],pending:false,retryAfter:0};
    studioWaveDetails.set(slot,entry);
  }
  const visibleDuration=end-start;
  const resolution=Math.max(1,Math.min(2400,pixels));
  const needsDetail=media.duration/media.peaks.length>visibleDuration/resolution*2;
  const requiredStep=Math.max(1/8000,visibleDuration/resolution)*1.5;
  const following=!audio.paused&&state.followPlayhead&&timelinePanPointer===null;
  const ahead=following?Math.min(media.duration,end+visibleDuration*.25):end;
  const covered=entry.details.some(detail=>
    detail.start<=start+1e-4&&detail.end+1e-4>=ahead&&detail.step<=requiredStep);
  if(needsDetail&&!covered&&!entry.pending&&performance.now()>=entry.retryAfter&&visibleDuration>0){
    entry.pending=true;
    const requestStart=Math.max(0,start-visibleDuration*.5);
    const requestEnd=Math.min(media.duration,end+visibleDuration);
    const bins=Math.min(6000,Math.ceil(resolution*(requestEnd-requestStart)/visibleDuration));
    const query=new URLSearchParams({start:requestStart,end:requestEnd,bins});
    api(`/api/projects/${pid}/waveform/${source}?${query}`).then(result=>{
      if(studioWaveDetails.get(slot)!==entry)return;
      entry.details=[result,...entry.details].slice(0,6);
    }).catch(()=>{entry.retryAfter=performance.now()+4000;}).finally(()=>{
      entry.pending=false;
      if(studioWaveDetails.get(slot)===entry&&state.project?.id===pid)scheduleTimelineDraw();
    });
  }
  return entry.details;
}
function studioWaveBounds(details,media,start,end){
  if(start<0||start>=media.duration)return [0,0];
  const pixelDuration=end-start;
  end=Math.min(end,media.duration);
  let detail=null;
  for(const candidate of details||[]){
    if(candidate.start<=start+1e-4&&candidate.end+1e-4>=end&&
       candidate.step<=Math.max(1/8000,pixelDuration)*3&&
       (!detail||candidate.step<detail.step))detail=candidate;
  }
  if(detail){
    const first=Math.max(0,Math.floor((start-detail.start)/detail.step));
    const last=Math.min(detail.max.length-1,Math.ceil((end-detail.start)/detail.step)-1);
    let low=detail.min[first]??0,high=detail.max[first]??0;
    for(let i=first+1;i<=last;i++){low=Math.min(low,detail.min[i]);high=Math.max(high,detail.max[i]);}
    return [low,high];
  }
  // Overview peaks represent a much longer interval when zoomed in. Leave the
  // center line visible in missing regions until their measured extrema arrive.
  if(media.duration/media.peaks.length>pixelDuration*3)return [0,0];
  const first=Math.floor(start/media.duration*media.peaks.length),last=Math.min(media.peaks.length-1,Math.floor(end/media.duration*media.peaks.length));
  let peak=media.peaks[first]||0;
  for(let i=first+1;i<=last;i++)peak=Math.max(peak,media.peaks[i]);
  return [-peak,peak];
}
let studioWaveRenderCache=null;
function buildStudioWaveRenderCache({width,H,dpr,start,secondsPerPixel,lanes,hidden}){
  const background=document.createElement('canvas'),played=document.createElement('canvas');
  background.width=played.width=Math.max(1,Math.round(width*dpr));
  background.height=played.height=Math.max(1,Math.round(H*dpr));
  const base=background.getContext('2d'),active=played.getContext('2d');
  for(const context of [base,active])context.setTransform(dpr,0,0,dpr,0,0);
  base.fillStyle='#121416';base.fillRect(0,0,width+1,H);
  for(const {slot,lane,color,media,detail} of lanes){
    const top=lane*H/3,mid=top+H/6,amplitude=Math.max(4,H/6-3);
    base.strokeStyle='#2a2f3a';
    base.beginPath();
    base.moveTo(0,mid);base.lineTo(width,mid);
    base.moveTo(0,top);base.lineTo(width,top);
    base.stroke();
    if(!media?.peaks?.length||hidden[lane])continue;
    const silhouette=new Path2D(),lower=[];
    for(let pixel=0;pixel<=Math.ceil(width);pixel++){
      const t=start+pixel*secondsPerPixel,next=t+secondsPerPixel;
      const source=slot==='reference'?mapTime(t,true):t,end=slot==='reference'?mapTime(next,true):next;
      const [low,high]=studioWaveBounds(detail,media,source,end);
      const upper=mid-high*amplitude;
      if(pixel===0)silhouette.moveTo(pixel,upper);
      else silhouette.lineTo(pixel,upper);
      lower.push(mid-low*amplitude);
    }
    for(let pixel=lower.length-1;pixel>=0;pixel--)silhouette.lineTo(pixel,lower[pixel]);
    silhouette.closePath();
    base.fillStyle=studioWaveColors.inactive;base.fill(silhouette);
    active.fillStyle=color;active.fill(silhouette);
  }
  return {background,played};
}
function studioWaveforms(range,playhead){
  const canvas=$('waveformCanvas'),rect=canvas.getBoundingClientRect();
  if(!state.project||!rect.width||!rect.height||!state.showWaves)return;
  const W=rect.width,H=rect.height,dpr=devicePixelRatio||1,G=timelineGutter(W),P=W-G;
  if(P<=0)return;
  if(canvas.width!==Math.round(W*dpr)||canvas.height!==Math.round(H*dpr)){
    canvas.width=Math.round(W*dpr);canvas.height=Math.round(H*dpr);
  }
  const ctx=canvas.getContext('2d');ctx.setTransform(dpr,0,0,dpr,0,0);
  const controls=$('waveTrackControls');
  if(controls.style.width!==G+'px')controls.style.width=G+'px';
  const hidden=state.hiddenWaveLanes||new Set();
  const lanes=[['beat',0,studioWaveColors.beat,state.project.beat],['reference',1,studioWaveColors.ref,state.project.vocals||state.project.reference],['backing',2,studioWaveColors.backing,state.project.backing]]
    .map(([slot,lane,color,media])=>({slot,lane,color,media,detail:media?.peaks?.length&&!hidden.has(slot)?studioWaveDetail(slot,media,range,P):null}));
  const hiddenFlags=lanes.map(({slot})=>hidden.has(slot));
  const signature=[state.project.id,W,H,dpr,G,range.span,JSON.stringify(state.project.anchors||[]),studioWaveColors.inactive,...hiddenFlags,...lanes.map(item=>item.color)].join('|');
  const cache=studioWaveRenderCache;
  const covered=cache&&range.start>=cache.start&&range.start<=cache.lastStart;
  const sameSources=cache?.signature===signature&&lanes.every((item,index)=>cache.media[index]===item.media&&cache.details[index]===item.detail);
  const secondsPerPixel=range.span/P;
  if(!covered||!sameSources){
    // Rasterize past the visible edges so following playback only shifts the cached image.
    const margin=range.limit>0?range.span/4:0;
    const start=Math.max(0,range.start-margin/2);
    const lastStart=Math.min(range.limit,range.start+margin);
    const width=Math.ceil(P+(lastStart-start)/secondsPerPixel)+1;
    studioWaveRenderCache={
      signature,start,lastStart,
      media:lanes.map(item=>item.media),details:lanes.map(item=>item.detail),
      ...buildStudioWaveRenderCache({width,H,dpr,start,secondsPerPixel,lanes,hidden:hiddenFlags})
    };
  }
  const {start,background,played}=studioWaveRenderCache;
  const sourceX=(range.start-start)/secondsPerPixel*dpr;
  const cursor=G+(playhead-range.start)/secondsPerPixel;
  ctx.clearRect(0,0,W,H);
  ctx.fillStyle='#1c1c1f';ctx.fillRect(0,0,G,H);
  ctx.strokeStyle='#2a2f3a';ctx.beginPath();
  for(let lane=0;lane<3;lane++){
    const top=lane*H/3;ctx.moveTo(0,top);ctx.lineTo(G,top);
  }
  ctx.stroke();
  ctx.drawImage(background,sourceX,0,P*dpr,background.height,G,0,P,H);
  const playedWidth=Math.min(P,cursor-G);
  if(playedWidth>0)ctx.drawImage(played,sourceX,0,playedWidth*dpr,played.height,G,0,playedWidth,H);
  if(cursor>=G&&cursor<=W){ctx.fillStyle='#ffffff';ctx.fillRect(cursor,0,2,H);}
}
