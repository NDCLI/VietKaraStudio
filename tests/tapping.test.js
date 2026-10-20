const test=require('node:test');
const assert=require('node:assert/strict');
const {TapTiming,tapPreviewTarget,smoothTapProgress}=require('../static/tap_timing.js');
const tap=(clock,time,wall=time*1000)=>{const item=clock.down(time,false,wall);clock.up(time+.05,wall+50);return item;};

test('hold uses beat time from keydown to keyup and ignores repeat',()=>{
  const c=new TapTiming(2);
  assert.equal(c.down(32.4),null);assert.equal(c.down(32.7,true),null);
  assert.deepEqual(c.up(33.2),{index:0,start:32.4,end:33.2});
  c.down(33.4);assert.deepEqual(c.up(33.8),{index:1,start:33.4,end:33.8});
  assert.equal(c.done,true);
});
test('quick taps and holds alternate in one session without a mode switch',()=>{
  const c=new TapTiming(3);
  assert.equal(tap(c,4),null);assert.equal(c.index,0);
  assert.deepEqual(c.down(4.5),{index:0,start:4,end:4.5});
  assert.deepEqual(c.up(5.1),{index:1,start:4.5,end:5.1});
  assert.equal(tap(c,5.4),null);
  assert.deepEqual(c.finish(5.8),{index:2,start:5.4,end:5.8});assert.equal(c.done,true);
});
test('extra tap closes last word without creating an extra token',()=>{
  const c=new TapTiming(1);tap(c,2);
  assert.deepEqual(tap(c,3),{index:0,start:2,end:3});assert.equal(c.armed,null);assert.equal(c.index,1);
});
test('Backspace returns to previous committed word and drops an uncommitted tap',()=>{
  const c=new TapTiming(5,2);tap(c,9);tap(c,9.4);
  assert.equal(c.index,3);assert.deepEqual(c.back(),{index:2,start:9,end:9.4});
  assert.equal(c.index,2);assert.equal(c.armed,null);
});
test('backwards or stopped clocks cannot create invalid held words',()=>{
  const c=new TapTiming(2);c.down(10,false,1000);
  assert.equal(c.up(9,1300),null);assert.equal(c.index,0);assert.equal(c.armed,null);
  c.down(10.2,false,1400);assert.deepEqual(c.up(10.4,1700),{index:0,start:10.2,end:10.4});
});
test('hold detection uses real elapsed time at slow playback',()=>{
  const c=new TapTiming(1);c.down(3,false,1000);
  assert.deepEqual(c.up(3.1,1200),{index:0,start:3,end:3.1});assert.equal(c.done,true);
});
test('pause or seek cancels an unfinished tap and release cannot record it',()=>{
  const c=new TapTiming(2);tap(c,5);c.cancel();
  assert.equal(c.up(8),null);assert.equal(c.finish(8),null);assert.equal(c.index,0);
  c.down(9);assert.deepEqual(c.up(9.4),{index:0,start:9,end:9.4});
});
test('very close consecutive presses do not advance twice',()=>{
  const c=new TapTiming(2);c.down(1,false,1000);c.up(1.005,1005);
  assert.equal(c.down(1.01,false,1010),null);assert.equal(c.index,0);
  assert.deepEqual(c.down(1.3,false,1300),{index:0,start:1,end:1.3});
});
test('live preview follows the pending Space press and keeps future words unsung',()=>{
  const c=new TapTiming(3),done=new Map();
  assert.equal(tapPreviewTarget(c,done,0,50),0);
  c.down(50);const a=tapPreviewTarget(c,done,0,50.1),b=tapPreviewTarget(c,done,0,50.3);
  assert.ok(a>0&&b>a&&b<1);
  assert.equal(tapPreviewTarget(c,done,1,55),0);
  const word=c.up(50.4);done.set(word.index,word);
  assert.equal(tapPreviewTarget(c,done,0,50.4),1);
  assert.deepEqual(word,{index:0,start:50,end:50.4});
});
test('preview settling is gradual and does not change recorded timestamps',()=>{
  const first=smoothTapProgress(.7,1,1/60),next=smoothTapProgress(first,1,1/60);
  assert.ok(first>.7&&first<1&&next>first&&next<1);
  assert.equal(smoothTapProgress(.3,1,0),.3);
});
