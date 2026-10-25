'use strict';
let exportMenuRevision=0;
async function refreshExportMenu(){
  const revision=++exportMenuRevision,p=state.project,s=p.settings;
  for(const [id,key] of [['exportFormat','render_format'],['exportQuality','render_quality'],['exportFps','render_fps'],['exportResolution','resolution'],['exportAspect','aspect']])$(id).value=s[key]??({render_quality:'high',render_fps:'source',render_format:'mp4'}[key]);
  $('exportFolder').value=p.export_directory||'';
  $('exportBeatName').textContent=p.beat?.name||'Chưa nạp beat';
  $('exportInventory').textContent='Đang kiểm tra file và thiết lập…';
  const info=await api(endpoint('/render-info'));if(revision!==exportMenuRevision)return;
  const o=info.output,bg=info.background,source=bg.source;
  const background=bg.kind==='video'?`Video gốc · ${source?`${source.width} × ${source.height} · ${Number(source.fps).toFixed(2)} fps`:'không đọc được nguồn'}`:bg.kind==='image'?'Ảnh đã nạp':'Màu nền '+s.background;
  const animation=$('lineAnimation').selectedOptions[0]?.textContent||s.line_animation;
  const countdown=$('countdownStyle').selectedOptions[0]?.textContent||s.countdown;
  const rows=[['Video',`${o.width} × ${o.height} · ${Number(o.fps).toFixed(2)} fps · ${o.format}`],
    ['Thời lượng',time(info.duration,true)],['Nền',background],['Lời',`${info.words} từ · ${info.lines} câu · ${s.singing_mode==='duet'?'Song ca · màu theo vai':'Đơn ca'}`],
    ['Font & viền',`${s.font} · cỡ ${s.font_size} (khung 1080p)${s.italic?' · nghiêng':''} · viền ${s.outline_width} · bóng ${s.shadow}`],
    ['Hiệu ứng chữ',`${animation} · quét theo từng từ${s.sung_gradient?' · gradient':''} · mờ biên ${s.sweep_feather} · sáng viền ${s.sweep_glow}`],
    ['Tiêu đề',s.title_enabled&&s.title_text?`${s.title_text} · ${s.title_start}–${s.title_end}s · ${s.title_enter} / ${s.title_exit}`:'Tắt'],
    ['Đếm ngược',s.countdown==='none'?'Tắt':countdown+' · đầu bài / sau nhạc dạo'],
    ['Logo',info.logo?`${p.logo_name} · ${s.logo_size}% · độ rõ ${s.logo_opacity}% · ${$('logoEffect').selectedOptions[0]?.textContent||s.logo_effect}`:'Tắt / chưa có ảnh'],
    ['Âm thanh',`Beat gốc · ${o.audio}`],['Thư mục',info.folder]];
  $('exportInventory').innerHTML='<dl>'+rows.map(([k,v])=>`<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join('')+'</dl><details><summary>Không đưa vào video</summary><p>'+info.excluded.map(esc).join(' · ')+'</p></details><p class="hint">Video được dựng từ beat gốc; file xuất chỉ gồm video đã ghép chữ, logo và nền.</p>';
  $('exportNotice').textContent=[...info.errors,...info.warnings].join('\n');$('exportNotice').classList.toggle('export-error',info.errors.length>0);
  $('renderBtn').disabled=info.errors.length>0||!!state.job;
  $('renderBtn').textContent='Dựng '+(s.render_format||'mp4').toUpperCase()+' · '+(s.resolution==='4k'?'4K':'1080p')+' · '+({standard:'Tiêu chuẩn',high:'Chất lượng cao',maximum:'Rất cao'}[s.render_quality]||'Chất lượng cao');
}
function initExportMenu(){
  for(const [id,key] of [['exportFormat','render_format'],['exportQuality','render_quality'],['exportFps','render_fps'],['exportResolution','resolution'],['exportAspect','aspect']])$(id).onchange=wrap(async()=>{
    if(state.job)return;state.project.settings[key]=$(id).value;renderStyle();await saveNow();await refreshExportMenu();
  });
  async function folder(pick){
    if(state.job)return;
    $('pickExportFolder').disabled=true;$('saveExportFolder').disabled=true;
    try{
      await saveNow();const result=await api(endpoint('/export-folder'),{method:'POST',body:pick?{pick:true}:{folder:$('exportFolder').value}});
      if(result.cancelled)return;state.project.export_directory=result.folder;await refreshExportMenu();
    }finally{$('pickExportFolder').disabled=false;$('saveExportFolder').disabled=false;}
  }
  $('pickExportFolder').onclick=wrap(()=>folder(true));$('saveExportFolder').onclick=wrap(()=>folder(false));
  $('exportFolder').onchange=wrap(()=>folder(false));
}
