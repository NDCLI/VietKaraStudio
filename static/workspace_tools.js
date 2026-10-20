'use strict';
function initWorkspaceTools(){
  $('videoReferenceBtn').onclick=wrap(async()=>{
    if(state.job||state.manualTiming)return;
    if(state.project.reference&&state.project.words.length&&!confirm('Dùng âm thanh video thay bản hát sẽ xóa mốc từ cũ. Tiếp tục?'))return;
    await saveNow();followJob(await api(endpoint('/video-reference'),{method:'POST',body:{}}));
  });
  $('videoSeparateBtn').onclick=wrap(async()=>{
    if(state.job||state.manualTiming)return;
    await saveNow();document.querySelector('[data-page="separation"]').click();
    $('separationResult').hidden=true;$('separationRun').disabled=true;
    try{
      const result=await waitUtilityJob(await api(endpoint('/separate'),{method:'POST',body:{source:'background',device:$('devicePreference').value}}),'separationStatus');
      if(result){$('separatedBeatPreview').src=result.beat;$('separatedBeatDownload').href=result.beat;$('separatedVocalPreview').src=result.vocals;$('separatedVocalDownload').href=result.vocals;$('separationResult').hidden=false;}
    }finally{$('separationRun').disabled=false;}
  });
  document.querySelectorAll('[data-page]').forEach(b=>b.onclick=wrap(async()=>{
    if(state.manualTiming)return toast('Kết thúc phiên đồng bộ trước khi chuyển tab.');
    if(!state.project)return;
    await saveNow();audio.pause();$('separatedBeatPreview').pause();$('separatedVocalPreview').pause();
    document.querySelectorAll('[data-app-page]').forEach(p=>p.hidden=p.dataset.appPage!==b.dataset.page);
    document.querySelectorAll('[data-page]').forEach(p=>p.classList.toggle('on',p===b));
    if(b.dataset.page==='settings')await refreshModels();
    if(b.dataset.page==='karaoke')drawTimeline();
  }));
  $('separationRun').onclick=wrap(async()=>{
    if(state.job)return toast('Đang có tác vụ xử lý.');
    const file=$('separationInput').files[0];if(!file)throw Error('Chọn bài nhạc cần tách.');
    const body=new FormData();body.append('file',file);body.append('device',$('separationDevice').value);
    $('separationRun').disabled=true;$('separationStatus').textContent='Đang nạp bài nhạc…';
    try{
      const job=await api(endpoint('/separate'),{method:'POST',body});
      $('separationResult').hidden=true;
      const result=await waitUtilityJob(job,'separationStatus');
      if(result){
        $('separatedBeatPreview').src=result.beat;$('separatedBeatDownload').href=result.beat;
        $('separatedVocalPreview').src=result.vocals;$('separatedVocalDownload').href=result.vocals;
        $('separationResult').hidden=false;
      }
    }finally{$('separationRun').disabled=false;}
  });
  $('refreshModels').onclick=wrap(refreshModels);
  $('modelList').onclick=wrap(async event=>{
    const button=event.target.closest('[data-download-model]');if(!button)return;
    if(state.job)return toast('Đang có tác vụ xử lý.');
    button.disabled=true;
    try{await waitUtilityJob(await api(endpoint('/models/'+button.dataset.downloadModel),{method:'POST',body:{}}));}
    finally{await refreshModels();}
  });
  $('presetStrip').onclick=wrap(async event=>{
    const button=event.target.closest('[data-quick-preset]');if(button)await applyPresetNow(button.dataset.quickPreset);
  });
  $('quickImportPreset').onclick=()=>$('importPresetInput').click();
  $('quickExportPreset').onclick=()=>$('exportPresetBtn').click();
  $('quickDeletePreset').onclick=()=>$('deletePresetBtn').click();
}
async function waitUtilityJob(job,statusId){
  busy(job);
  try{
    while(['running','queued'].includes(job.state)){
      if(statusId)$(statusId).textContent=job.message;
      await new Promise(resolve=>setTimeout(resolve,750));
      job=await api('/api/jobs/'+job.id);busy(job);
    }
    if(job.state==='error')throw Error(job.message);
    if(statusId)$(statusId).textContent=job.state==='done'?'Hoàn tất.':'Đã hủy tác vụ.';
    return job.state==='done'?job.result:null;
  }finally{busy(null);}
}
async function refreshModels(){
  $('modelList').textContent='Đang kiểm tra model trên máy…';
  const models=await api('/api/models');
  $('modelList').innerHTML=models.map(m=>`<div class="model-item"><div><strong>${m.id==='demucs'?'Demucs · tách nhạc':'Whisper '+esc(m.id)}</strong><span class="${m.ready?'green-text':'muted'}">${m.ready?'Đã có trên máy':'Chưa tải'}</span></div><p class="model-location">${m.ready?(m.location==='cache'?'Đang dùng cache có sẵn · không tải trùng':'Đang dùng model trong thư mục phần mềm'):'Chưa có model'}</p><label class="hint">${m.ready?'Đường dẫn đang dùng':'Thư mục tải / chép model'}</label><code>${esc(m.path)}</code>${m.ready&&m.download_path!==m.path?`<label class="hint">Thư mục tải mới / chép model thủ công</label><code>${esc(m.download_path)}</code>`:''}<button class="secondary" data-download-model="${esc(m.id)}" ${state.job?'disabled':''}>${m.ready?'Kiểm tra / tải bổ sung':'Tải model'}</button></div>`).join('');
}
async function applyPresetNow(id){
  if(state.job||state.manualTiming)return toast('Kết thúc tác vụ hoặc phiên đồng bộ trước khi đổi preset.');
  const preset=state.presets.find(p=>p.id===id);if(!preset)return;
  state.presetId=id;state.project.settings={...state.project.settings,...preset.settings,font_size:110,display_mode:'karaoke'};
  renderStyle();renderInlinePresets();await saveNow();$('presetDialog').close();
  toast('Đã áp dụng '+preset.name+' · cỡ chữ '+state.project.settings.font_size+'.');
}
function renderInlinePresets(){
  if(!state.project)return;
  const chosen=state.presets.find(p=>Object.entries(p.settings).every(([k,v])=>state.project.settings[k]===v));
  if(chosen)state.presetId=chosen.id;
  $('presetStrip').innerHTML=state.presets.map(p=>`<button data-quick-preset="${esc(p.id)}" class="quick-preset ${p.id===chosen?.id?'on':''}" title="${esc(p.name)} · cỡ ${p.settings.font_size}"><span style="font-family:${esc(p.settings.font)};color:${p.settings.sung};font-style:${p.settings.italic?'italic':'normal'};-webkit-text-stroke:1px ${p.settings.outline_sung};text-shadow:2px 2px ${p.settings.shadow_color}">Nhạc</span><small>${esc(p.name)}</small></button>`).join('');
  $('quickExportPreset').disabled=!state.presetId;
  const selected=state.presets.find(p=>p.id===state.presetId);
  $('quickDeletePreset').disabled=!selected||selected.builtin;
}
