'use strict';
const UPDATE_PREF='vietkara_auto_update';
let updateRequest=null,updateCheckRequest=null,updateReady=false,updateVersion='',updateInfo=null;
let updateApplying=false,updatePromptTimer=null,updateDismissedVersion='';
function updateEnabled(){return localStorage.getItem(UPDATE_PREF)!=='off';}
function updateStatus(text){$('updateStatus').textContent=text;}
function updateBlocked(){return !!(state.job||state.renderJob||state.manualTiming);}
function renderUpdateAction(){
  const button=$('updateAction');button.hidden=!updateVersion;button.textContent='Cập nhật';button.disabled=!!updateRequest;
  $('updateDialogInstall').disabled=updateApplying;
  $('updateDialogLater').disabled=updateApplying;
}
function showUpdatePrompt(result,manual=false){
  if(!result?.update_available||(!manual&&updateDismissedVersion===result.version))return;
  if(updateBlocked()||document.querySelector('dialog[open]:not(#updateDialog)')){
    clearTimeout(updatePromptTimer);
    updatePromptTimer=setTimeout(()=>showUpdatePrompt(result,manual),1500);
    return;
  }
  $('updateDialogVersion').textContent='VietKara Studio '+result.version;
  $('updateDialogNotes').textContent=result.notes||'Bản cập nhật mới đã sẵn sàng.';
  $('updateDialogStatus').textContent=updateReady?'Đã tải xong. Cập nhật sẽ lưu project và khởi động lại app.':'Cập nhật sẽ tải bản vá, lưu project và khởi động lại app.';
  renderUpdateAction();
  if(!$('updateDialog').open)$('updateDialog').showModal();
}
async function downloadUpdate(){
  if(updateRequest)return updateRequest;
  updateStatus('Đang tải và kiểm tra bản vá…');
  updateRequest=api('/api/updates/download',{method:'POST',body:{}}).then(result=>{
    updateReady=!!result.ready;updateVersion=result.version||updateVersion;
    if(!updateReady)throw Error('Bản cập nhật chưa sẵn sàng. Hãy thử lại.');
    updateStatus(`Đã tải và xác minh bản vá ${updateVersion}.`);return result;
  }).finally(()=>{updateRequest=null;renderUpdateAction();});
  renderUpdateAction();return updateRequest;
}
async function checkForUpdates(automatic=false,prompt=false){
  if(updateCheckRequest)return updateCheckRequest;
  updateStatus('Đang kiểm tra bản vá mới…');
  updateCheckRequest=(async()=>{
    try{
      const result=await api('/api/updates/check');
      if(!result.supported||!result.update_available){
        updateVersion='';updateReady=false;updateInfo=null;
        updateStatus(result.supported?`${result.message} · Phiên bản ${result.installed_version}`:result.message);
        renderUpdateAction();return result;
      }
      updateInfo=result;updateVersion=result.version;updateReady=!!result.ready;
      updateStatus(updateReady?`Đã sẵn sàng cài bản vá ${updateVersion}.`:`Có bản vá ${updateVersion} · khoảng ${(result.size/1024/1024).toFixed(0)} MB.`);
      renderUpdateAction();
      if(automatic||prompt)showUpdatePrompt(result,prompt);
      return result;
    }catch(error){updateStatus('Chưa kiểm tra được bản vá. Hãy thử lại khi có mạng.');if(!automatic)handleError(error);return null;}
    finally{updateCheckRequest=null;}
  })();
  return updateCheckRequest;
}
async function installUpdateFromPrompt(){
  if(updateApplying)return;
  if(updateBlocked()){$('updateDialogStatus').textContent='Hãy kết thúc tác vụ hoặc đồng bộ trước khi cập nhật.';return;}
  updateApplying=true;renderUpdateAction();
  let applied=false;
  try{
    $('updateDialogStatus').textContent=updateReady?'Đang lưu project…':'Đang tải và kiểm tra bản cập nhật…';
    if(!updateReady)await downloadUpdate();
    if(updateBlocked())throw Error('Hãy kết thúc tác vụ hoặc đồng bộ trước khi cập nhật.');
    await saveNow();
    $('updateDialogStatus').textContent='Đang khởi động lại để cài bản cập nhật…';
    updateStatus('Đang khởi động lại để cài bản vá…');
    await api('/api/updates/apply',{method:'POST',body:{}});
    applied=true;
  }catch(error){
    $('updateDialogStatus').textContent=error.message||'Chưa cập nhật được. Bạn có thể thử lại hoặc chọn Để sau.';
    updateStatus(error.message||'Chưa cập nhật được.');
  }finally{if(!applied)updateApplying=false;renderUpdateAction();}
}
window.refreshUpdateStatus=()=>checkForUpdates(false);
function initUpdates(){
  const toggle=$('autoUpdateEnabled');toggle.checked=updateEnabled();
  toggle.onchange=()=>{localStorage.setItem(UPDATE_PREF,toggle.checked?'on':'off');updateStatus(toggle.checked?'Tự động thông báo bản mới khi mở app.':'Đã tắt tự động kiểm tra cập nhật.');};
  $('checkUpdates').onclick=wrap(()=>checkForUpdates(false,true));
  $('updateAction').onclick=()=>showUpdatePrompt(updateInfo,true);
  $('updateDialogLater').onclick=()=>{if(updateApplying)return;updateDismissedVersion=updateVersion;clearTimeout(updatePromptTimer);$('updateDialog').close();};
  $('updateDialog').addEventListener('cancel',event=>{if(updateApplying)event.preventDefault();else updateDismissedVersion=updateVersion;});
  $('updateDialogInstall').onclick=installUpdateFromPrompt;
  if(updateEnabled())setTimeout(()=>checkForUpdates(true),1200);
  else updateStatus('Đã tắt tự động kiểm tra cập nhật. Bấm “Kiểm tra ngay” khi muốn xem bản mới.');
}
