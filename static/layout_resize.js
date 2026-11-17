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
  fitStudioPreview();drawTimeline();
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
  const observer=new ResizeObserver(()=>{fitStudioPreview();drawTimeline();});
  observer.observe(document.querySelector('.editor-column'));observer.observe($('waveformCanvas'));
  addEventListener('resize',applyStudioLayout);document.addEventListener('fullscreenchange',()=>{fitStudioPreview();studioFrame();});
  $('waveformCanvas').onpointerdown=event=>{
    if(state.manualTiming||!state.project)return;
    const rect=event.currentTarget.getBoundingClientRect(),g=timelineGutter(rect.width),range=timelineRange();
    seekBeat(Math.max(0,Math.min(range.duration,range.start+(event.clientX-rect.left-g)/(rect.width-g)*range.span)));drawTimeline();
  };
  applyStudioLayout();
}
// Interpolate the measured peak envelope; never fabricate extra audio samples.
function studioWavePeak(peaks,index){
  if(index<0||index>=peaks.length)return 0;
  const i=Math.floor(index),u=index-i,a=peaks[i],b=peaks[Math.min(i+1,peaks.length-1)];
  const before=peaks[Math.max(0,i-1)],after=peaks[Math.min(i+2,peaks.length-1)],delta=b-a;
  const tangent=(left,right)=>left*right>0?2*left*right/(left+right):0;
  const m0=tangent(a-before,delta),m1=tangent(delta,after-b),u2=u*u,u3=u2*u;
  return Math.max(0,Math.min(1,(2*u3-3*u2+1)*a+(u3-2*u2+u)*m0+(-2*u3+3*u2)*b+(u3-u2)*m1));
}
function studioWaveforms(){
  const canvas=$('waveformCanvas'),rect=canvas.getBoundingClientRect();if(!state.project||!rect.width||!rect.height||!state.showWaves)return;
  const W=rect.width,H=rect.height,dpr=devicePixelRatio||1,G=timelineGutter(W),P=W-G;
  if(canvas.width!==Math.round(W*dpr)||canvas.height!==Math.round(H*dpr)){canvas.width=Math.round(W*dpr);canvas.height=Math.round(H*dpr);}
  const ctx=canvas.getContext('2d');ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,W,H);
  $('waveTrackControls').style.width=G+'px';
  const range=timelineRange(),x=t=>G+(t-range.start)/range.span*P;
  const playhead=beatNow();
  ctx.fillStyle='#121416';ctx.fillRect(0,0,W,H);
  for(const [slot,lane,color,title] of [['beat',0,studioWaveColors.beat,'NHẠC NỀN'],['reference',1,studioWaveColors.ref,'GIỌNG HÁT'],['backing',2,studioWaveColors.backing,'GIỌNG BÈ']]){
    const top=lane*H/3,mid=top+H/6,amplitude=Math.max(4,H/6-3);
    const m=slot==='reference'?(state.project.vocals||state.project.reference):state.project[slot];
    ctx.fillStyle='#1c1c1f';ctx.fillRect(0,top,G,H/3);
    ctx.strokeStyle='#2a2f3a';ctx.beginPath();ctx.moveTo(G,mid);ctx.lineTo(W,mid);ctx.moveTo(0,top);ctx.lineTo(W,top);ctx.stroke();
    if(!m?.peaks?.length||state.hiddenWaveLanes?.has(slot))continue;
    ctx.save();ctx.beginPath();ctx.rect(G,top,P,H/3);ctx.clip();
    const envelope=[];
    for(let pixel=0;pixel<=Math.ceil(P);pixel++){
      const t=range.start+pixel/P*range.span,next=range.start+(pixel+1)/P*range.span;
      const source=slot==='reference'?mapTime(t,true):t,end=slot==='reference'?mapTime(next,true):next;
      const index=source/m.duration*m.peaks.length,endIndex=end/m.duration*m.peaks.length;
      let peak=studioWavePeak(m.peaks,index);
      // Preserve brief transients when several measured bins share one pixel.
      for(let bin=Math.max(0,Math.ceil(index));bin<Math.min(m.peaks.length,endIndex);bin++)peak=Math.max(peak,m.peaks[bin]);
      envelope.push(Math.max(.35,peak*amplitude));
    }
    ctx.beginPath();ctx.moveTo(G,mid-envelope[0]);
    for(let pixel=1;pixel<envelope.length;pixel++)ctx.lineTo(G+pixel,mid-envelope[pixel]);
    for(let pixel=envelope.length-1;pixel>=0;pixel--)ctx.lineTo(G+pixel,mid+envelope[pixel]);
    ctx.closePath();ctx.fillStyle=studioWaveColors.inactive;ctx.fill();
    ctx.save();ctx.beginPath();ctx.rect(G,top,Math.max(0,Math.min(P,x(playhead)-G)),H/3);ctx.clip();
    // The clip above starts a new path, so replay the silhouette for played audio.
    ctx.beginPath();ctx.moveTo(G,mid-envelope[0]);
    for(let pixel=1;pixel<envelope.length;pixel++)ctx.lineTo(G+pixel,mid-envelope[pixel]);
    for(let pixel=envelope.length-1;pixel>=0;pixel--)ctx.lineTo(G+pixel,mid+envelope[pixel]);
    ctx.closePath();ctx.fillStyle=color;ctx.fill();ctx.restore();
    ctx.restore();
  }
  const cursor=x(playhead);if(cursor>=G&&cursor<=W){ctx.fillStyle='#ffffff';ctx.fillRect(cursor,0,2,H);}
}
