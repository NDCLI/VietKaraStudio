'use strict';
let exportMenuRevision=0;
let completedExportFolder=null;
function exportButtonLabel(s){
  if(s.export_kind==='audio')return 'Xuất '+(s.audio_format||'wav').toUpperCase();
  return 'Dựng '+(s.render_format||'mp4').toUpperCase()+' · '+({ '4k':'4K','2k':'2K','1080p':'1080p' }[s.resolution]||'1080p')+' · '+({standard:'Tiêu chuẩn',high:'Cao',maximum:'Rất cao'}[s.render_quality]||'Cao');
}
function syncExportFields(){
  const s=state.project.settings,isAudio=s.export_kind==='audio',kind=isAudio?'audio':'video',select=$('exportFormat');
  $('ceKindSelect').value=kind;
  if(select.dataset.exportKind!==kind){
    select.innerHTML=(isAudio?['wav','mp3','flac','m4a']:['mp4','mov','mkv']).map(value=>`<option value="${value}">${value.toUpperCase()}</option>`).join('');
    select.dataset.exportKind=kind;
  }
  select.value=isAudio?s.audio_format||'wav':s.render_format||'mp4';
  $('exportResolutionLabel').hidden=isAudio;$('videoQualityRow').hidden=isAudio;$('previewRenderBtn').hidden=isAudio;
  $('exportAudio').closest('label').style.gridColumn=isAudio?'1 / -1':'';
  $('ceModeNote').textContent=isAudio?'Xuất âm thanh theo track đã chọn. WAV và FLAC giữ chất lượng không mất dữ liệu; MP3 và M4A có dung lượng gọn hơn.':'Xuất video karaoke theo độ phân giải và chất lượng đã chọn.';
  $('renderBtn').textContent=exportButtonLabel(s);
}
async function openCompletedExportFolder(){
  if(completedExportFolder)await api(completedExportFolder,{method:'POST',body:{}});
}
async function showExportResult(job){
  const result=job.result,isAudio=result.kind==='audio',url=result.audio||result.video;
  const match=url?.match(/\/exports\/([0-9a-f]{32})\//);
  completedExportFolder=match?`/api/projects/${job.project_id}/exports/${match[1]}/open-folder`:null;
  $('openExportFolder').disabled=!completedExportFolder;
  $('exportSavedPath').textContent=result.output_path?'Đã lưu: '+result.output_path:'';
  $('renderResult').querySelector('strong').textContent=result.preview?'Đã dựng xong đoạn thử':'Đã xuất '+(isAudio?'âm thanh ':'video ')+(result.format||'').toUpperCase();
  $('renderResult').hidden=false;$('exportDialog').showModal();
  toast(isAudio?'Đã xuất file âm thanh.':'Đã dựng xong video.');
  if($('ceOpenFolderCheck').checked&&!result.preview){try{await openCompletedExportFolder();}catch(error){handleError(error);}}
}
async function refreshExportMenu(){
  const revision=++exportMenuRevision,p=state.project,s=p.settings;
  syncExportFields();
  for(const [id,key] of [['exportAudio','audio_output'],['exportQuality','render_quality'],['exportFps','render_fps'],['exportResolution','resolution'],['exportAspect','aspect']])$(id).value=s[key]??({audio_output:'beat',render_quality:'high',render_fps:'source'}[key]);
  $('exportAudio').querySelector('[value="backing"]').disabled=p.backing_status!=='ready';
  $('exportFolder').value=p.export_directory||'';
  $('exportBeatName').textContent=p.beat?.name||'Chưa nạp beat';
  $('exportInventory').textContent='Đang kiểm tra file và thiết lập…';
  const info=await api(endpoint('/render-info'));if(revision!==exportMenuRevision)return;
  if(s.export_kind==='audio'){
    $('exportNotice').textContent=[...info.errors,...info.warnings].join('\n');$('exportNotice').classList.toggle('export-error',info.errors.length>0);
    $('exportChecks').hidden=true;$('exportInventory').textContent=info.output.audio+' · '+info.output.format;
    $('renderBtn').disabled=info.errors.length>0||!!state.job||!!state.renderJob;return;
  }
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
    ['Âm thanh',`${o.audio}${info.audio_quality?` · peak ${info.audio_quality.peak_dbfs} dBFS · RMS ${info.audio_quality.rms_dbfs} dBFS`:''}`],['Thư mục',info.folder]];
  $('exportInventory').innerHTML='<dl>'+rows.map(([k,v])=>`<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join('')+'</dl><details><summary>Không đưa vào video</summary><p>'+info.excluded.map(esc).join(' · ')+'</p></details>';
  $('exportNotice').textContent=[...info.errors,...info.warnings].join('\n');$('exportNotice').classList.toggle('export-error',info.errors.length>0);
  const checks=info.checks||[],box=$('exportChecks');box.hidden=!checks.length;
  box.innerHTML=checks.length?'<h3>Rà soát lời · bấm để tới vị trí</h3><div class="export-check-list">'+checks.slice(0,200).map(item=>`<button type="button" class="export-check-item" data-qa-word="${Number(item.word_index)}"><small>${time(mapTime(item.time),true)} · C${Number(item.line)+1}</small><span>${esc(item.message)}</span></button>`).join('')+'</div>'+(checks.length>200?`<p class="hint">Còn ${checks.length-200} điểm khác; danh sách đang hiển thị 200 điểm đầu.</p>`:''):'';
  const reviewLines = new Map();
  p.words.forEach((word, index) => {
    if (!reviewLines.has(word.line)) reviewLines.set(word.line, {index, unlocked:false, tokens:[]});
    const line = reviewLines.get(word.line);
    line.tokens.push(word.text);
    if (!word.locked) line.unlocked = true;
  });
  const pending = [...reviewLines.values()].filter(line => line.unlocked);
  if (pending.length) {
    box.hidden = false;
    box.insertAdjacentHTML('beforeend', `<details><summary>Câu chưa khóa (${pending.length})</summary><div class="export-check-list">${pending.map(({index,tokens}) => {
      const word = p.words[index];
      return `<button type="button" class="export-check-item" data-qa-word="${index}"><small>${time(mapTime(word.start),true)} · C${word.line+1}</small><span>${esc(tokens.join(" "))}</span></button>`;
    }).join('')}</div></details>`);
  }
  $('renderBtn').disabled=info.errors.length>0||!!state.job;
  $('previewRenderBtn').disabled=info.errors.length>0||!!state.job;
  $('renderBtn').textContent=exportButtonLabel(s);
}
function initExportMenu(){
  $('openExportFolder').onclick=wrap(openCompletedExportFolder);
  $('ceKindSelect').onchange=wrap(async()=>{
    state.project.settings.export_kind=$('ceKindSelect').value;syncExportFields();await saveNow();await refreshExportMenu();
  });
  $('exportChecks').onclick = event => {
    const button = event.target.closest('[data-qa-word]');
    if (!button) return;
    const index = Number(button.dataset.qaWord);
    const word = Number.isInteger(index) && state.project.words[index];
    if (!word) return;
    $('exportDialog').close();
    selectWord(index, false);
    seekBeat(Math.max(0, mapTime(word.start)));
  };
  for(const [id,key] of [['exportAudio','audio_output'],['exportFormat','render_format'],['exportQuality','render_quality'],['exportFps','render_fps'],['exportResolution','resolution'],['exportAspect','aspect']])$(id).onchange=wrap(async()=>{
    if(state.job)return;const setting=id==='exportFormat'&&state.project.settings.export_kind==='audio'?'audio_format':key;state.project.settings[setting]=$(id).value;renderStyle();await saveNow();await refreshExportMenu();
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
