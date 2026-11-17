'use strict';
function initWorkspaceTools(){
  const beatPreview=$('separatedBeatPreview');
  const separationPlayers=[beatPreview,$('separatedBeatBackingPreview'),$('separatedVocalPreview')];
  separationPlayers.forEach(player=>player.addEventListener('play',()=>separationPlayers.forEach(other=>{if(other!==player)other.pause();})));
  $('videoReferenceBtn').onclick=wrap(async()=>{
    if(state.job||state.manualTiming)return;
    if(state.project.reference&&state.project.words.length&&!confirm('Dùng âm thanh video thay bản hát sẽ xóa mốc từ cũ. Tiếp tục?'))return;
    await saveNow();followJob(await api(endpoint('/video-reference'),{method:'POST',body:{}}));
  });
  $('videoSeparateBtn').onclick=wrap(async()=>{
    if(state.job||state.manualTiming)return;
    if((state.project.beat||state.project.reference||state.project.words.length)&&!confirm('Tách video và dùng làm nguồn sẽ thay Beat, bản có vocal và các mốc lời hiện tại. Tiếp tục?'))return;
    await saveNow();audio.pause();$('karaokeSeparationStatus').textContent='Đang tách video thành Beat và Vocal…';
    const job=await api(endpoint('/separate'),{method:'POST',body:{source:'background',apply_to_project:true,device:'auto'}});
    const completed=await followJob(job);
    if(!completed?.result?.applied){$('karaokeSeparationStatus').textContent='Chưa áp dụng kết quả tách. Kiểm tra thông báo lỗi hoặc thử lại.';return;}
    if(state.project?.vocals_ready){setAudio('beat',0);$('karaokeSeparationStatus').textContent='Đã nạp Beat và bản có vocal. Bấm loa tại sóng nhạc để nghe từng bản.';toast('Đã tách và nạp nguồn ngay trong Karaoke.');}
  });
  document.querySelectorAll('[data-page]').forEach(b=>b.onclick=wrap(async()=>{
    if(state.manualTiming)return toast('Kết thúc phiên đồng bộ trước khi chuyển tab.');
    if(!state.project)return;
    await saveNow();audio.pause();separationPlayers.forEach(player=>player.pause());
    document.querySelectorAll('[data-app-page]').forEach(p=>p.hidden=p.dataset.appPage!==b.dataset.page);
    document.querySelectorAll('[data-page]').forEach(p=>{p.classList.toggle('on',p===b);if(p===b)p.setAttribute('aria-current','page');else p.removeAttribute('aria-current');});
    if(b.dataset.page==='settings'){await refreshModels();if(window.refreshUpdateStatus)window.refreshUpdateStatus();}
    if(b.dataset.page==='karaoke')drawTimeline();
  }));
  const separationButtons=[$('separationRun'),$('separationWithBackingRun')];
  const separationInputs=[$('separationInput'),$('separationYoutubeUrl'),$('separationBackingModel')];
  try{const saved=localStorage.getItem('vietkara_backing_model');if(['roformer','uvr-lite'].includes(saved))$('separationBackingModel').value=saved;}catch{}
  $('separationBackingModel').onchange=()=>{try{localStorage.setItem('vietkara_backing_model',$('separationBackingModel').value);}catch{}};
  $('separationInput').addEventListener('change',()=>{
    const file=$('separationInput').files[0];
    $('separationFileName').textContent=file?file.name:'MP3, WAV, FLAC, M4A, MP4 và nhiều định dạng khác';
    if(file)$('separationYoutubeUrl').value='';
  });
  $('separationYoutubeUrl').addEventListener('input',()=>{
    if($('separationYoutubeUrl').value.trim()){
      $('separationInput').value='';
      $('separationFileName').textContent='MP3, WAV, FLAC, M4A, MP4 và nhiều định dạng khác';
    }
  });
  async function runSeparation(mode){
    if(state.job||separationButtons.some(button=>button.disabled))return;
    const file=$('separationInput').files[0];
    const youtube=($('separationYoutubeUrl').value||'').trim();
    if(!file&&!youtube)throw Error('Chọn bài nhạc hoặc dán URL YouTube cần tách.');
    if(file&&youtube)throw Error('Chỉ chọn file hoặc URL YouTube, không dùng cả hai.');
    const backing_model=$('separationBackingModel').value;
    const body=youtube?{source:'youtube',url:youtube,device:'auto',mode,backing_model}:new FormData();
    if(file){body.append('file',file);body.append('device','auto');body.append('mode',mode);body.append('backing_model',backing_model);}
    separationButtons.forEach(button=>button.disabled=true);
    separationInputs.forEach(input=>input.disabled=true);
    $('separationActivity').dataset.state='running';
    $('separationProgress').hidden=false;
    $('separationProgress').removeAttribute('value');
    $('separationStatus').textContent=youtube?'Đang chuẩn bị tải YouTube…':'Đang nạp bài nhạc…';
    try{
      const job=await api(endpoint('/separate'),{method:'POST',body});
      separationPlayers.forEach(player=>player.pause());
      $('separationResult').hidden=true;
      const result=await waitUtilityJob(job,'separationStatus');
      if(result){
        const mixed=result.backing_mix;
        const mixedDownload=$('separatedBeatBackingDownload');
        $('separatedBeatDownload').href=result.beat;
        $('separatedVocalDownload').href=result.vocals;
        $('separatedVocalPreview').src=result.vocals+'?preview=1';
        const mixedPreview=$('separatedBeatBackingPreview');
        mixedPreview.hidden=!mixed;
        $('separatedBackingCard').classList.toggle('unavailable',!mixed);
        if(mixed){
          mixedPreview.src=mixed+'?preview=1';
          $('separatedBackingNote').textContent='Nhạc nền giữ lại giọng hát bè.';
          mixedDownload.href=mixed;
          mixedDownload.removeAttribute('aria-disabled');
        }else{
          mixedPreview.removeAttribute('src');mixedPreview.load();
          $('separatedBackingNote').textContent=result.backing_status==='weak'?'Chưa tách được bè rõ từ bản thu này.':'Chọn “Tách beat có bè” để tạo bản này.';
          mixedDownload.removeAttribute('href');
          mixedDownload.setAttribute('aria-disabled','true');
        }
        beatPreview.src=result.beat+'?preview=1';
        if(result.backing_status==='weak'){
          $('separationStatus').textContent='Không tách được bè rõ. Đã tạo beat không bè.';
        }
        $('separationResult').hidden=false;
      }
      $('separationActivity').dataset.state=result?'done':'idle';
    }catch(error){
      $('separationActivity').dataset.state='error';
      $('separationStatus').textContent=error.message||'Không tách được nhạc. Hãy thử lại.';
      throw error;
    }finally{separationButtons.forEach(button=>button.disabled=false);separationInputs.forEach(input=>input.disabled=false);$('separationProgress').hidden=true;}
  }
  $('separationRun').onclick=wrap(()=>runSeparation('beat'));
  $('separationWithBackingRun').onclick=wrap(()=>runSeparation('backing'));
  $('refreshModels').onclick=wrap(refreshModels);
  $('modelList').onclick=wrap(async event=>{
    const selBtn=event.target.closest('[data-select-model]');
    if(selBtn){
      const id=selBtn.dataset.selectModel,grp=selBtn.dataset.modelGroup;
      if(grp==='WHISPER'){
        localStorage.setItem('obs_active_whisper_model',id);
        if($('aiModel'))$('aiModel').value=id;
      }else if(grp==='BACKING'){
        localStorage.setItem('obs_active_backing_model',id);
        const mapVal={'backing-roformer':'roformer','mdx-kara-2':'mdx-kara-2','backing-uvr-lite':'uvr-lite'}[id]||'roformer';
        if($('separationBackingModel'))$('separationBackingModel').value=mapVal;
      }else if(grp==='INST'){
        localStorage.setItem('obs_active_inst_model',id);
        if($('separationInstModel'))$('separationInstModel').value=id;
      }
      await refreshModels();
      toast('Đã chọn model '+id);
      return;
    }
    const button=event.target.closest('[data-download-model]');if(!button)return;
    if(state.job)return toast('Đang có tác vụ xử lý.');
    button.disabled=true;
    try{await waitUtilityJob(await api(endpoint('/models/'+button.dataset.downloadModel),{method:'POST',body:{}}));}
    finally{await refreshModels();}
  });
  $('separationInstModel')?.addEventListener('change',e=>{
    localStorage.setItem('obs_active_inst_model',e.target.value);
  });
  $('separationBackingModel')?.addEventListener('change',e=>{
    const rev={'roformer':'backing-roformer','mdx-kara-2':'mdx-kara-2','uvr-lite':'backing-uvr-lite'}[e.target.value]||'backing-roformer';
    localStorage.setItem('obs_active_backing_model',rev);
  });
  $('presetStrip').onclick=wrap(async event=>{
    const button=event.target.closest('[data-quick-preset]');if(button)await applyPresetNow(button.dataset.quickPreset);
  });
  $('quickImportPreset').onclick=()=>$('importPresetInput').click();
  $('quickExportPreset').onclick=()=>$('exportPresetBtn').click();
  $('quickDeletePreset').onclick=()=>$('deletePresetBtn').click();
}
async function waitUtilityJob(job,statusId){
  const kind=job.kind;
  busy(job);
  try{
    while(['running','queued'].includes(job.state)){
      if(statusId)$(statusId).textContent=friendlyJobMessage(job);
      if(statusId==='separationStatus')$('separationProgress').value=Math.max(0,Math.min(100,Number(job.progress)||0));
      await new Promise(resolve=>setTimeout(resolve,750));
      job=await api('/api/jobs/'+job.id);busy(job);
    }
    if(job.state==='error')throw Error(job.message);
    if(statusId)$(statusId).textContent=job.state==='done'?'Hoàn tất.':'Đã hủy tác vụ.';
    return job.state==='done'?job.result:null;
  }finally{busy(null,kind);}
}
const EXTRA_MODEL_META={
  'mdx-inst-hq-1':{roleGroup:'INST',tag:'2 STEMS'},
  'mdx-inst-hq-2':{roleGroup:'INST',tag:'2 STEMS'},
  'mdx-inst-hq-3':{roleGroup:'INST',tag:'2 STEMS'},
  'mdx-kim-vocal-2':{roleGroup:'INST',tag:'2 STEMS'},
  'demucs':{roleGroup:'INST',label:'HTDemucs · Tách nhạc chuẩn',tag:'4 STEMS',desc:'Model tách nhạc nền & giọng hát chuẩn của hệ thống',pro:'Ổn định trên mọi thể loại nhạc, giữ nhịp và pha chuẩn để căn lời',con:'Đôi khi còn vương nhẹ tiếng bè vào vocal chính',use:'Tách nhạc nền (beat) và giọng hát (vocal) cân bằng'},
  'backing-roformer':{roleGroup:'BACKING',label:'Mel-RoFormer Karaoke · Tách bè cao cấp',tag:'GIỮ BÈ CHUẨN',desc:'Tách riêng giọng hát chính và giữ lại giọng bè (backing vocal) chất lượng cao',pro:'Phân tách giọng chính và giọng bè cực kỳ sạch, giữ bè dày và rõ',con:'Nặng hơn, thời gian xử lý lâu hơn trên CPU',use:'Làm karaoke chuyên nghiệp cần giữ giọng bè chuẩn như bản gốc'},
  'mdx-kara-2':{roleGroup:'BACKING',tag:'2 STEMS · GIỮ BÈ'},
  'backing-uvr-lite':{roleGroup:'BACKING',label:'UVR Karaoke Lite · Tách bè nhẹ',tag:'NHANH · NHẸ',desc:'Model tách giọng bè gọn nhẹ cho máy cấu hình vừa',pro:'Tốc độ tách bè nhanh, nhẹ máy, tốn ít bộ nhớ',con:'Độ chi tiết của bè kém hơn Mel-RoFormer một chút ở bài phối phức tạp',use:'Máy không có GPU rời mạnh hoặc cần tách bè nhanh'},
  'small':{roleGroup:'WHISPER',label:'Whisper small',tag:'NHẬN LỜI NHANH',desc:'Nhận diện lời bài hát tốc độ cao, nhẹ máy',pro:'Chạy rất nhanh, tốn ít VRAM/RAM, phù hợp máy yếu hoặc CPU',con:'Độ chính xác tiếng Việt thấp hơn ở các đoạn hát luyến hoặc nhạc ồn',use:'Nháp nhanh hoặc bài hát phát âm rõ ràng, máy cấu hình thấp'},
  'medium':{roleGroup:'WHISPER',label:'Whisper medium',tag:'CÂN BẰNG',desc:'Cân bằng giữa tốc độ và độ chính xác tiếng Việt',pro:'Nhận diện tiếng Việt ổn định, thời gian xử lý vừa phải',con:'Cần khoảng 2–3 GB VRAM/RAM, đôi khi nhầm từ đồng âm',use:'Mặc định khuyên dùng cho đa số bài karaoke tiếng Việt'},
  'large-v3-turbo':{roleGroup:'WHISPER',label:'Whisper large-v3-turbo',tag:'CHÍNH XÁC CAO',desc:'Bản Whisper thế hệ mới chính xác nhất và tối ưu tốc độ',pro:'Nhận diện tiếng Việt, tiếng Anh và nhạc trẻ mới rất chuẩn, bắt nhịp từ sát',con:'Dung lượng model lớn hơn (~1.6 GB)',use:'Bài hát khó, nhiều luyến láy, rap hoặc cần căn mốc từ chuẩn nhất'}
};
async function refreshModels(){
  $('modelList').textContent='Đang kiểm tra model trên máy…';
  const models=await api('/api/models');
  const activeInst=localStorage.getItem('obs_active_inst_model')||'mdx-inst-hq-1';
  const activeBacking=localStorage.getItem('obs_active_backing_model')||'backing-roformer';
  const activeWhisper=localStorage.getItem('obs_active_whisper_model')||$('aiModel')?.value||'medium';
  if($('separationInstModel'))$('separationInstModel').value=activeInst;
  if($('separationBackingModel')){
    $('separationBackingModel').value={'backing-roformer':'roformer','mdx-kara-2':'mdx-kara-2','backing-uvr-lite':'uvr-lite'}[activeBacking]||'roformer';
  }

  const enriched=models.map(m=>{
    const extra=EXTRA_MODEL_META[m.id]||{};
    return {...m,...extra,roleGroup:extra.roleGroup||'INST',label:extra.label||m.label||m.id,tag:extra.tag||'2 STEMS'};
  });

  const groups=[
    {key:'INST',title:'1. MODEL TÁCH NHẠC NỀN & VOCAL CHÍNH (CHỌN 1 MODEL)',activeId:activeInst},
    {key:'BACKING',title:'2. MODEL TÁCH GIỌNG BÈ KARAOKE (CHỌN 1 MODEL)',activeId:activeBacking},
    {key:'WHISPER',title:'3. MODEL NHẬN DIỆN LỜI & TỰ CĂN TỪ · WHISPER AI (CHỌN 1 MODEL)',activeId:activeWhisper},
  ];

  $('modelList').innerHTML=groups.map(g=>{
    const list=enriched.filter(m=>m.roleGroup===g.key);
    if(!list.length)return '';
    const readyCount=list.filter(m=>m.ready).length;
    return `<details class="mdx-group" open>
      <summary class="mdx-group-head"><strong style="color:#10b981;font-size:11px;letter-spacing:.5px">${esc(g.title)}</strong><span class="small muted">${readyCount}/${list.length} đã tải</span></summary>
      <div class="mdx-cards">${list.map(m=>{
        const isSelected=m.id===g.activeId;
        return `<div class="mdx-card ${isSelected?'active':''}">
          <div class="mdx-card-top">
            <div class="mdx-title-row">
              <strong>${esc(m.label)}</strong>
              ${isSelected?'<span class="mdx-badge active">ĐANG DÙNG</span>':''}
              <span class="mdx-badge stems">${esc(m.tag||'2 STEMS')}</span>
              <span class="mdx-badge ${m.ready?'ready':'missing'}">${m.ready?'✓ ĐÃ TẢI':'CHƯA TẢI'}</span>
            </div>
            <div style="display:flex;gap:6px;align-items:center">
              <button type="button" class="secondary small" data-download-model="${esc(m.id)}" ${state.job?'disabled':''}>${m.ready?'Kiểm tra / tải lại':'Tải model'}</button>
              <button type="button" class="mdx-select-btn ${isSelected?'on':''}" data-select-model="${esc(m.id)}" data-model-group="${esc(g.key)}">${isSelected?'✓ Đang dùng':'Chọn'}</button>
            </div>
          </div>
          <p class="mdx-desc">${esc(m.desc||'')}</p>
          <ul class="mdx-specs">
            <li>✓ <b>Mạnh:</b> ${esc(m.pro||'')}</li>
            <li>⚠ <b>Yếu:</b> ${esc(m.con||'')}</li>
            <li>💡 <b>Nên dùng khi:</b> ${esc(m.use||'')}</li>
          </ul>
          <details class="model-manual" style="margin-top:4px;font-size:11px"><summary class="muted" style="cursor:pointer">Đường dẫn trên máy (${m.ready?(m.location==='cache'?'cache hệ thống':'thư mục app'):'chưa tải'})</summary><code style="display:block;margin-top:4px">${esc(m.path)}</code></details>
        </div>`;
      }).join('')}</div>
    </details>`;
  }).join('');
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
