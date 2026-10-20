'use strict';
const studioLayoutDefaults={left:255,right:295,waves:120,lyrics:136};
let studioLayoutSize={...studioLayoutDefaults};
function applyStudioLayout(){
  const root=$('karaokePage'),width=root.clientWidth;
  studioLayoutSize.left=Math.max(170,Math.min(studioLayoutSize.left,Math.max(170,width-500)));
  studioLayoutSize.right=Math.max(230,Math.min(studioLayoutSize.right,Math.max(230,width-studioLayoutSize.left-250)));
  const maxBottom=Math.max(230,Math.min(innerHeight*.62,innerHeight-380));
  studioLayoutSize.waves=Math.max(80,Math.min(studioLayoutSize.waves,maxBottom-110));
  studioLayoutSize.lyrics=Math.max(110,Math.min(studioLayoutSize.lyrics,maxBottom-studioLayoutSize.waves));
  for(const [key,value] of Object.entries(studioLayoutSize))root.style.setProperty('--layout-'+key,value+'px');
  for(const [id,value] of [['leftColumnGrip',studioLayoutSize.left],['rightColumnGrip',studioLayoutSize.right],['studioHeightGrip',studioLayoutSize.waves+studioLayoutSize.lyrics],['waveformHeightGrip',studioLayoutSize.waves]]){
    $(id).setAttribute('aria-valuemin','0');$(id).setAttribute('aria-valuemax',String(Math.max(innerWidth,innerHeight)));
    $(id).setAttribute('aria-valuenow',String(Math.round(value)));$(id).setAttribute('aria-valuetext',Math.round(value)+' px');
  }
  fitStudioPreview();drawTimeline();
}
function saveStudioLayout(){try{localStorage.setItem('obs_studio_layout_v1',JSON.stringify(studioLayoutSize));}catch{}}
function fitStudioPreview(){
  const column=document.querySelector('.editor-column'),stage=$('preview');
  if(!column||!stage||document.fullscreenElement===stage||!column.clientWidth||!column.clientHeight)return;
  const ratio=stage.classList.contains('portrait')?9/16:16/9;
  const width=Math.max(1,Math.min(column.clientWidth,Math.max(1,column.clientHeight-36)*ratio));
  stage.style.width=width.toFixed(2)+'px';stage.style.height=(width/ratio).toFixed(2)+'px';
}
function initLayoutResize(){
  try{const saved=JSON.parse(localStorage.getItem('obs_studio_layout_v1'));for(const key of Object.keys(studioLayoutDefaults))if(Number.isFinite(saved?.[key]))studioLayoutSize[key]=saved[key];}catch{}
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
function studioWaveforms(){
  const canvas=$('waveformCanvas'),rect=canvas.getBoundingClientRect();if(!state.project||!rect.width||!rect.height||!state.showWaves)return;
  const W=rect.width,H=rect.height,dpr=devicePixelRatio||1,G=timelineGutter(W),P=W-G;
  if(canvas.width!==Math.round(W*dpr)||canvas.height!==Math.round(H*dpr)){canvas.width=Math.round(W*dpr);canvas.height=Math.round(H*dpr);}
  const ctx=canvas.getContext('2d');ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,W,H);
  const range=timelineRange(),x=t=>G+(t-range.start)/range.span*P;
  ctx.fillStyle='#121416';ctx.fillRect(0,0,W,H);
  for(const [slot,lane,color,title] of [['beat',0,'#ef9039','BEAT GỐC'],['reference',1,'#20c89c','BẢN HÁT']]){
    const top=lane*H/2,mid=top+H/4,amplitude=Math.max(8,H/4-10),m=state.project[slot];
    ctx.fillStyle='#1c1c1f';ctx.fillRect(0,top,G,H/2);ctx.font='bold 9px Segoe UI';ctx.fillStyle=color;ctx.fillText(title,9,mid+3);
    ctx.strokeStyle='#30373b';ctx.beginPath();ctx.moveTo(G,mid);ctx.lineTo(W,mid);ctx.moveTo(0,top);ctx.lineTo(W,top);ctx.stroke();
    if(!m?.peaks?.length)continue;
    ctx.save();ctx.beginPath();ctx.rect(G,top,P,H/2);ctx.clip();
    for(let pixel=0;pixel<P;pixel+=2){
      const t=range.start+pixel/P*range.span,source=slot==='reference'?mapTime(t,true):t,index=Math.floor(source/m.duration*m.peaks.length);
      if(index<0||index>=m.peaks.length)continue;
      const a=m.peaks[index]*amplitude;ctx.fillStyle=t<=beatNow()?color:'#5b626d';ctx.fillRect(G+pixel,mid-a,1.4,Math.max(1,a*2));
    }
    ctx.restore();
  }
  const cursor=x(beatNow());if(cursor>=G&&cursor<=W){ctx.fillStyle='#eef5ee';ctx.fillRect(cursor,0,1.5,H);}
}
