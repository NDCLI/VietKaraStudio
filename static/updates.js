'use strict';
const UPDATE_PREF='vietkara_auto_update';
let updateRequest=null,updateReady=false,updateVersion='';
function updateEnabled(){return localStorage.getItem(UPDATE_PREF)!=='off';}
function updateStatus(text){$('updateStatus').textContent=text;}
function renderUpdateAction(){const button=$('updateAction');button.hidden=!updateVersion;button.textContent=updateReady?'Cài đặt và khởi động lại':'Tải bản vá';button.disabled=!!updateRequest;}
async function downloadUpdate(auto=false){
  if(updateRequest)return updateRequest;
  updateStatus('Đang tải và kiểm tra bản vá…');renderUpdateAction();
  updateRequest=api('/api/updates/download',{method:'POST',body:{}}).then(result=>{
    updateReady=!!result.ready;updateVersion=result.version||updateVersion;
    updateStatus(`Đã tải và xác minh bản vá ${updateVersion}. Bấm “Cài đặt và khởi động lại” để áp dụng.`);
    renderUpdateAction();if(!auto)toast(`Đã tải xong bản vá ${updateVersion}.`);return result;
  }).catch(error=>{updateStatus(error.message||'Chưa tải được bản vá.');if(!auto)handleError(error);throw error;})
    .finally(()=>{updateRequest=null;renderUpdateAction();});
  return updateRequest;
}
async function checkForUpdates(automatic=false){
  if(updateRequest)return updateRequest;
  if(automatic&&updateEnabled())updateStatus('Đang kiểm tra bản vá mới…');
  try{
    const result=await api('/api/updates/check');
    if(!result.supported){updateVersion='';updateReady=false;updateStatus(result.message);renderUpdateAction();return result;}
    if(!result.update_available){updateVersion='';updateReady=false;updateStatus(`${result.message} · Phiên bản ${result.installed_version}`);renderUpdateAction();return result;}
    updateVersion=result.version;updateReady=!!result.ready;
    updateStatus(updateReady?`Đã sẵn sàng cài bản vá ${updateVersion}.`:`Có bản vá ${updateVersion} · khoảng ${(result.size/1024/1024).toFixed(0)} MB.`);
    renderUpdateAction();
    if(automatic&&updateEnabled()&&!updateReady){await downloadUpdate(true);toast(`Có bản vá ${updateVersion}; đã tải xong, có thể khởi động lại để áp dụng.`);}
    return result;
  }catch(error){updateStatus('Chưa kiểm tra được bản vá. Hãy thử lại khi có mạng.');if(!automatic)handleError(error);return null;}
}
window.refreshUpdateStatus=()=>checkForUpdates(false);
function initUpdates(){
  const toggle=$('autoUpdateEnabled');toggle.checked=updateEnabled();
  toggle.onchange=()=>{localStorage.setItem(UPDATE_PREF,toggle.checked?'on':'off');updateStatus(toggle.checked?'Tự động kiểm tra và tải bản vá khi mở app.':'Đã tắt tự động kiểm tra cập nhật.');};
  $('checkUpdates').onclick=wrap(()=>checkForUpdates(false));
  $('updateAction').onclick=wrap(async()=>{
    if(!updateReady){await downloadUpdate(false);return;}
    if(state.job||state.manualTiming)throw Error('Hãy kết thúc tác vụ và đồng bộ trước khi cập nhật.');
    await saveNow();if(!confirm(`VietKara sẽ đóng và mở lại để cài bản vá ${updateVersion}. Tiếp tục?`))return;
    updateStatus('Đang khởi động lại để cài bản vá…');
    await api('/api/updates/apply',{method:'POST',body:{}});
  });
  if(updateEnabled())setTimeout(()=>checkForUpdates(true),1200);
  else updateStatus('Đã tắt tự động kiểm tra cập nhật. Bấm “Kiểm tra ngay” khi muốn xem bản mới.');
}
