const test=require('node:test');
const assert=require('node:assert/strict');
const {timelineResize}=require('../static/timeline_edit.js');
const a={start:1,end:2,line:0},b={start:2,end:3,line:0},c={start:3,end:4,line:0};
test('linked shared right and left boundaries adjust only the touching neighbour',()=>{
  assert.deepEqual(timelineResize(b,a,c,'end',.5,true,10),{start:2,end:3.5,nextStart:3.5});
  assert.deepEqual(timelineResize(b,a,c,'start',-.5,true,10),{start:1.5,end:3,previousEnd:1.5});
  assert.deepEqual(a,{start:1,end:2,line:0});
});
test('independent resize creates a rest without changing the next word',()=>{
  assert.deepEqual(timelineResize(b,a,c,'end',-.5,false,10),{start:2,end:2.5});
  assert.deepEqual(timelineResize(b,a,c,'start',.5,false,10),{start:2.5,end:3});
});
test('long rests and different lines are never linked',()=>{
  assert.deepEqual(timelineResize(b,a,{...c,start:3.3},'end',-.5,true,10),{start:2,end:2.5});
  assert.deepEqual(timelineResize(b,{...a,line:1},c,'start',.2,true,10),{start:2.2,end:3});
});
test('linked drag preserves minimum duration on both sides and beat limits',()=>{
  assert.deepEqual(timelineResize(b,a,c,'end',9,true,10),{start:2,end:3.95,nextStart:3.95});
  assert.deepEqual(timelineResize(b,a,c,'start',-9,true,10),{start:1.05,end:3,previousEnd:1.05});
  assert.equal(timelineResize(b,null,null,'end',20,false,4).end,4);
  assert.equal(timelineResize(b,null,null,'start',-20,false,4).start,0);
});
