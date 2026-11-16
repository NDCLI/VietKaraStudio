'use strict';

function renderBackingControls(){
  const project=state.project;
  const status=project?.backing_status;
  const blocked=!!state.job||!!state.manualTiming;
  const prepare=$('prepareBackingBtn'),toggle=$('toggleBackingBtn');
  const ready=status==='ready';
  prepare.disabled=blocked||!project?.beat||!(project.reference||project.vocals_ready)||status==='weak';
  prepare.textContent={review:'Nghe thử giọng bè',stale:'Căn lại giọng bè',ready:'Đã xác nhận giọng bè'}[status]||'Tách giọng bè';
  prepare.hidden=ready;
  $('backingStatus').textContent={
    review:'Nghe thử và xác nhận nếu bè rõ và vào đúng nhịp.',
    stale:'Beat hoặc mốc đồng bộ đã đổi. Hãy căn lại bè.',
    weak:'Chưa tách được bè rõ.',
  }[status]||'';
  toggle.disabled=blocked||!ready;
  const active=ready&&state.track==='beat'&&!!state.backingListening;
  toggle.textContent=active?'Bè · Bật':'Bè · Tắt';
  toggle.title=ready?'Bật hoặc tắt giọng bè khi nghe Beat':'Mở Nguồn media để tách và nghe thử giọng bè';
  toggle.setAttribute('aria-pressed',String(active));
  $('previewAudioLabel').textContent=active?'BEAT + BÈ':'BEAT GỐC';
}

function refreshBackingStatus(project){
  state.project.backing_status=project.backing_status;
  state.project.backing_vocals=project.backing_vocals;
  if(project.backing_status!=='ready'&&state.backingListening){
    state.backingListening=false;
    if(state.track==='beat')setAudio('beat',state.pendingAudio?.position??beatNow(),state.pendingAudio?.resume??!audio.paused);
  }
  renderBackingControls();
}

function initBackingVocals(){
  const dialog=$('backingReviewDialog'),sample=$('backingReviewAudio');
  let reviewMix;
  function showReview(){
    audio.pause();
    reviewMix=state.project.backing_vocals.mix;
    sample.src=endpoint('/audio/beat_backing')+'?v='+reviewMix;
    dialog.showModal();
  }
  dialog.addEventListener('close',()=>sample.pause());
  $('prepareBackingBtn').onclick=wrap(async()=>{
    if(state.job||state.manualTiming)return;
    await saveNow();
    if(state.project.backing_status==='review')return showReview();
    await followJob(await api(endpoint('/backing'),{method:'POST',body:{device:'auto'}}));
    if(state.project.backing_status==='review')showReview();
  });
  for(const [id,accepted] of [['acceptBackingBtn',true],['rejectBackingBtn',false]]){
    $(id).onclick=wrap(async()=>{
      const project=await api(endpoint('/backing/review'),{method:'POST',body:{accepted,mix:reviewMix}});
      refreshBackingStatus(project);
      dialog.close();
    });
  }
  $('toggleBackingBtn').onclick=wrap(async()=>{
    if(state.job||state.manualTiming)return;
    if(state.saveTimer)await saveNow();
    else await state.saveChain;
    if(state.project.backing_status!=='ready')return;
    const pending=state.pendingAudio;
    const position=pending?.position??beatNow();
    const playing=pending?.resume??!audio.paused;
    state.backingListening=!(state.track==='beat'&&state.backingListening);
    setAudio('beat',position,playing);
    drawTimeline();
  });
}
