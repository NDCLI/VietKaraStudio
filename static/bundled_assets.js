'use strict';
let sourceCatalog=null;
function sourceFontText(text,family){
  text=String(text).normalize('NFC');
  const encoding=sourceCatalog?.bundled.find(f=>f.family===family)?.encoding;
  const table=sourceCatalog?.encodings[encoding];
  return table?Array.from(text,c=>table[c]??c).join(''):text;
}
function renderSourceAssets(){
  if(!state.project||!sourceCatalog)return;
  const s=state.project.settings;
  $('fontSample').textContent=sourceFontText('Tiếng Việt: Trường xưa, nắng mới, yêu đời.',s.font);
  for(const [id,family] of [['fontEncodingNote',s.font],['titleFontEncodingNote',s.title_font]]){
    const encoding=sourceCatalog.bundled.find(f=>f.family===family)?.encoding;
    $(id).textContent=encoding==='tcvn3'?'Font TCVN3: tự chuyển mã khi hiển thị. Chọn bản có đuôi H cho chữ IN HOA.':
      encoding==='vni'?'Font VNI: tự chuyển mã khi hiển thị; lời gốc vẫn giữ Unicode.':
      encoding==='other'?'Font đặc biệt: hãy kiểm tra mẫu chữ trước khi xuất.':'';
    $(id).hidden=!$(id).textContent;
  }
  for(const button of document.querySelectorAll('[data-role-asset]')){
    const active=s[button.dataset.setting]===button.dataset.roleAsset;
    button.classList.toggle('on',active);button.setAttribute('aria-pressed',String(active));
  }
}
async function initSourceAssets(){
  try{
    sourceCatalog=await api('/api/fonts?catalog=1');
    const current=state.project?.settings.font||'Arial',titleCurrent=state.project?.settings.title_font||'Arial';
    const groups=[['unicode','Font mới · Unicode'],['vni','Font mới · VNI'],['tcvn3','Font mới · TCVN3'],['other','Font mới · Đặc biệt']];
    const option=name=>`<option value="${esc(name)}">${esc(name)}</option>`;
    const markup=groups.map(([encoding,label])=>{
      const names=[...new Set(sourceCatalog.bundled.filter(f=>f.encoding===encoding).map(f=>f.family))].sort((a,b)=>a.localeCompare(b));
      return names.length?`<optgroup label="${label}">${names.map(option).join('')}</optgroup>`:'';
    }).join('')+`<optgroup label="Font hệ thống · Tiếng Việt">${sourceCatalog.system.map(option).join('')}</optgroup>`;
    for(const [id,value] of [['fontName',current],['titleFont',titleCurrent]]){
      $(id).innerHTML=markup;
      if(!Array.from($(id).options).some(o=>o.value===value))$(id).add(new Option(value+' · font project',value));
      $(id).value=value;
    }
    const total=new Set(sourceCatalog.bundled.map(f=>f.family)).size;
    $('fontCountNote').textContent=`Đã thêm ${sourceCatalog.bundled.length} file font (${total} họ font) từ SOURCE.`;
    $('titleFontCountNote').textContent=`Có đủ ${total} họ font mới cho tiêu đề.`;
    const settings={male:'role_a_icon',female:'role_b_icon',all:'role_all_icon'},labels={male:'Nam',female:'Nữ',all:'Song ca'};
    $('roleIconChoices').innerHTML=Object.entries(sourceCatalog.icons).map(([role,icons])=>
      `<div class="role-art-group"><span>${labels[role]}</span><div>${icons.map(icon=>`<button type="button" class="role-art-choice" data-setting="${settings[role]}" data-role-asset="${esc(icon.file)}" aria-label="${labels[role]} · ${esc(icon.label)}" title="${esc(icon.label)}"><img src="${icon.url}" alt=""><small>${esc(icon.label)}</small></button>`).join('')}</div></div>`).join('');
    $('roleIconChoices').onclick=event=>{const button=event.target.closest('[data-role-asset]');if(button)setStyleValue(button.dataset.setting,button.dataset.roleAsset);};
    renderSourceAssets();
  }catch(error){
    $('fontCountNote').textContent='Chưa nạp được tài nguyên mới. Hãy tải lại ứng dụng.';
    $('titleFontCountNote').textContent='Chưa nạp được danh sách font mới.';
    console.error(error);
  }
}
