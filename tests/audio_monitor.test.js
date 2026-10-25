const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');

function monitor(){
  const elements={waveTrackControls:{innerHTML:''},listenTrack:{value:''},playbackRate:{value:'1'},seek:{},totalTime:{}};
  const audio={paused:false,muted:false,currentTime:12,duration:30,played:0,pause(){this.paused=true;},play(){this.paused=false;this.played++;return Promise.resolve();},removeAttribute(){},load(){}};
  const ctx={audio,state:{project:{updated_at:1,beat:{duration:30},reference:{duration:40},vocals_ready:true},track:'beat'},
    $:id=>elements[id],wrap:fn=>fn,beatNow:()=>12,mapTime:t=>t+3,endpoint:s=>'/project'+s,
    time:String,previewFrame(){},drawTimeline(){},handleError:e=>{throw e;}};
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(__dirname,'../static/song_title.js'),'utf8'),ctx);
  const app=fs.readFileSync(path.join(__dirname,'../static/app.js'),'utf8');
  vm.runInContext(app.slice(app.indexOf('function setAudio('),app.indexOf('async function togglePlay')),ctx);
  ctx.initWaveTrackControls();
  return {ctx,audio,elements,click:slot=>elements.waveTrackControls.onclick({target:{closest:()=>({dataset:{monitorTrack:slot}})}})};
}

test('switch to isolated vocal preserves mapped position and resumes playback',async()=>{
  const m=monitor();await m.click('vocals');
  assert.equal(m.ctx.state.track,'vocals');assert.match(m.audio.src,/audio\/vocals/);
  m.audio.onloadedmetadata();assert.equal(m.audio.currentTime,15);assert.equal(m.audio.played,1);
  assert.match(m.elements.waveTrackControls.innerHTML,/Nghe Beat/);
  assert.match(m.elements.waveTrackControls.innerHTML,/Tắt tiếng Vocal riêng/);
});
test('speaker button mutes without losing playback clock, then unmutes',async()=>{
  const m=monitor();await m.click('beat');assert.equal(m.audio.muted,true);assert.equal(m.audio.currentTime,12);
  await m.click('beat');assert.equal(m.audio.muted,false);assert.equal(m.audio.currentTime,12);
});
test('rapid track changes replace pending metadata callback',async()=>{
  const m=monitor();await m.click('reference');const first=m.audio.onloadedmetadata;
  await m.click('vocals');assert.notEqual(m.audio.onloadedmetadata,first);
  m.audio.onloadedmetadata();assert.equal(m.ctx.state.track,'vocals');assert.equal(m.audio.currentTime,15);
});
