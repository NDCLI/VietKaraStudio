'use strict';
const studioIcons={
  music:'<path d="M9 18V5l12-2v13M9 8l12-2"/><ellipse cx="6" cy="18" rx="3" ry="3"/><ellipse cx="18" cy="16" rx="3" ry="3"/>',
  mic:'<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8"/>',
  headphones:'<path d="M3 14v-3a9 9 0 0 1 18 0v3"/><rect x="3" y="12" width="4" height="9" rx="2"/><rect x="17" y="12" width="4" height="9" rx="2"/>',
  settings:'<path d="m10 2 4 0 .7 3 2.5 1.5 3-.8 2 3.5-2.2 2.1v2.9l2.2 2.1-2 3.5-3-.8-2.5 1.5-.7 3h-4l-.7-3L6.8 19l-3 .8-2-3.5L4 14.2v-2.9L1.8 9.2l2-3.5 3 .8L9.3 5Z"/><circle cx="12" cy="12" r="3"/>',
  video:'<rect x="2" y="5" width="14" height="14" rx="3"/><path d="m16 10 6-4v12l-6-4"/>',
  image:'<rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8" cy="8" r="1"/><path d="m3 16 5-5 5 6 4-4 4 4"/>',
  sparkles:'<path d="m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5ZM20 2v4M18 4h4"/>',
  download:'<path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"/>',
  badge:'<path d="m12 3 3 2 4 .5.5 4 2 2.5-2 3-.5 4-4 .5-3 2-3-2-4-.5-.5-4-2-3 2-2.5.5-4L9 5Z"/><path d="m8 12 3 3 5-6"/>',
  text:'<path d="M3 5h18M12 5v16M8 21h8M3 9V5m18 4V5"/>',
  folder:'<path d="M3 7V4h6l3 3h9v13H3Z"/>',
  plus:'<path d="M12 5v14M5 12h14"/>',
  expand:'<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>',
};
function studioIcon(name){return `<svg class="ui-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${studioIcons[name]||studioIcons.music}</svg>`;}
function initBranding(){
  const wave='<svg class="brand-wave" viewBox="0 0 32 32" aria-hidden="true"><g stroke-linecap="round" stroke-width="2.5"><path d="M4 14v4M9 9v14" stroke="#b7dc48"/><path d="M14 5v22M19 9v14" stroke="#27deb0"/><path d="M24 12v8M29 15v2" stroke="#f0b24a"/></g></svg>';
  document.querySelector('.rail-brand').innerHTML=wave;
  document.querySelector('.brand-icon').innerHTML=wave;
  for(const [page,icon] of [['karaoke','mic'],['separation','headphones'],['settings','settings']])document.querySelector(`[data-page="${page}"]`).innerHTML=studioIcon(icon);
  for(const el of document.querySelectorAll('[data-icon]'))el.innerHTML=studioIcon(el.dataset.icon);
  document.querySelector('.reference-card .upload-icon').innerHTML=studioIcon('mic');
  document.querySelector('.beat-card .upload-icon').innerHTML=studioIcon('headphones');
  $('projectsBtn').innerHTML=studioIcon('folder')+' Project';$('newBtn').innerHTML=studioIcon('plus')+' Bài mới';
  $('exportBtn').innerHTML=studioIcon('download')+' Xuất karaoke';$('fullscreenBtn').innerHTML=studioIcon('expand');
  $('previewLogo').onload=()=>updateLogoFrame(beatNow());
}
function updateLogoFrame(t){
  const p=state.project,s=p.settings,img=$('previewLogo'),stage=$('preview');
  img.hidden=!s.logo_enabled||!p.logo_name;if(img.hidden)return;
  const url=endpoint('/logo')+'?v='+(p.logo_version||p.updated_at);
  if(img.getAttribute('data-source')!==url){img.setAttribute('data-source',url);img.src=url;}
  const ratio=p.logo_image?p.logo_image.width/p.logo_image.height:(img.naturalWidth/img.naturalHeight||1);
  const w=Math.min(stage.clientWidth*(s.logo_size??12)/100,stage.clientHeight*.4*ratio),h=w/ratio;
  let y=(stage.clientHeight-h)*(s.logo_y??4)/100;
  if(s.logo_effect==='float')y=Math.max(0,Math.min(stage.clientHeight-h,y+8*stage.clientHeight/1080*Math.sin(Math.PI*t/2)));
  let alpha=(s.logo_opacity??100)/100;
  if(s.logo_effect==='fade')alpha*=Math.min(1,Math.max(0,t));
  if(s.logo_effect==='pulse')alpha*=.8+.2*Math.sin(Math.PI*t/2);
  Object.assign(img.style,{width:w+'px',height:h+'px',left:(stage.clientWidth-w)*(s.logo_x??96)/100+'px',top:y+'px',opacity:String(alpha)});
}
