const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync(require('node:path').join(__dirname,'../static/app.js'),'utf8');
const context=vm.createContext({});
vm.runInContext(source.slice(source.indexOf('function friendlyJobMessage('),source.indexOf('function busy(')),context);
test('separation status hides model and device diagnostics',()=>{
  for(const message of ['Demucs · NVIDIA GTX 1060: 50%…','CUDA lỗi; đang chạy CPU','Đang tải model tách bè · 100 MB','Đang tách vocal bằng Demucs · RTX 3060…']){
    const text=context.friendlyJobMessage({kind:'separate',state:'running',progress:20,message});
    assert.doesNotMatch(text,/Demucs|NVIDIA|GTX|RTX|CUDA|CPU|model|MB/i);
    assert.match(text,/Đang/);
  }
  assert.equal(context.friendlyJobMessage({kind:'separate',state:'running',progress:20,message:'Đang tải nguồn YouTube…'}),'Đang tải nhạc từ YouTube…');
  assert.equal(context.friendlyJobMessage({kind:'separate',state:'running',progress:70,message:'Some internal model details'}),'Đang xử lý giọng bè…');
  assert.equal(context.friendlyJobMessage({kind:'separate',state:'done',message:'internal'}),'Tách nhạc hoàn tất.');
});
