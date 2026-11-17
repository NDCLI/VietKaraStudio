const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');

function monitor(){
  const elements={waveTrackControls:{innerHTML:''},listenTrack:{value:''},playbackRate:{value:'1'},seek:{},totalTime:{}};
  const audio={addEventListener(){},getAttribute(){return this.src||'';},paused:false,muted:false,currentTime:12,duration:30,played:0,pause(){this.paused=true;},play(){this.paused=false;this.played++;return Promise.resolve();},removeAttribute(){},load(){}};
  const ctx={audio,state:{project:{updated_at:1,beat:{duration:30},reference:{duration:40},vocals_ready:true,backing_ready:true},track:'beat'},
    $:id=>elements[id],wrap:fn=>fn,beatNow:()=>12,mapTime:t=>t+3,endpoint:s=>'/project'+s,
    Audio:function(){return {paused:true,currentTime:0,play(){this.paused=false;return Promise.resolve();},pause(){this.paused=true;}};},location:{origin:'http://localhost'},time:String,previewFrame(){},drawTimeline(){},handleError:e=>{throw e;}};
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(__dirname,'../static/song_title.js'),'utf8'),ctx);
  const app=fs.readFileSync(path.join(__dirname,'../static/app.js'),'utf8');
  vm.runInContext(app.slice(app.indexOf('function setAudio('),app.indexOf('async function togglePlay')),ctx);
  ctx.initWaveTrackControls();
  return {ctx,audio,elements,click:(slot,action='mute')=>elements.waveTrackControls.onclick({target:{closest:selector=>selector===(action==='solo'?'[data-monitor-track]':'[data-mute-track]')?{dataset:action==='solo'?{monitorTrack:slot}:{muteTrack:slot}}:null}})};
}

test('direct track switch preserves mapped position and resumes playback',async()=>{
  const m=monitor();m.ctx.setAudio('reference',12,true);
  assert.equal(m.ctx.state.track,'reference');assert.match(m.audio.src,/audio\/reference/);
  m.audio.onloadedmetadata();assert.equal(m.audio.currentTime,15);assert.equal(m.audio.played,1);
  assert.match(m.elements.waveTrackControls.innerHTML,/NHẠC NỀN/);
  assert.match(m.elements.waveTrackControls.innerHTML,/GIỌNG HÁT/);
  assert.match(m.elements.waveTrackControls.innerHTML,/GIỌNG BÈ/);
});
test('mixer mute preserves playback clock and resumes the selected track',async()=>{
  const m=monitor();await m.click('beat');assert.equal(m.ctx.state.trackMuted.beat,true);assert.equal(m.audio.currentTime,12);
  await m.click('beat');assert.equal(m.ctx.state.trackMuted.beat,false);assert.equal(m.audio.currentTime,12);
});
test('solo selects one track and a second click restores the mix',async()=>{
  const m=monitor();await m.click('vocals','solo');
  assert.equal(m.ctx.state.trackMuted.vocals,false);assert.equal(m.ctx.state.trackMuted.beat,true);assert.equal(m.ctx.state.trackMuted.backing,true);
  await m.click('vocals','solo');assert.equal(m.ctx.state.trackMuted.beat,false);assert.equal(m.ctx.state.trackMuted.backing,false);
});
test('rapid direct track changes replace pending metadata callback',async()=>{
  const m=monitor();m.ctx.setAudio('reference',12,true);const first=m.audio.onloadedmetadata;
  m.ctx.setAudio('beat',12,true);assert.notEqual(m.audio.onloadedmetadata,first);
  m.audio.onloadedmetadata();assert.equal(m.ctx.state.track,'beat');assert.equal(m.audio.currentTime,12);
});
