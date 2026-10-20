'use strict';
// Pure beat-clock state machine, shared by the keyboard UI and the Node tests.
class TapTiming {
  constructor(count,index=0){
    if(!Number.isInteger(count)||count<1)throw Error('Cấu hình gõ nhịp không hợp lệ.');
    this.count=count;this.index=Math.max(0,Math.min(count-1,index));this.history=[];this.cancel();
  }
  cancel(){this.armed=null;this.pressing=false;this.pressedAt=null;}
  commit(end){
    if(this.armed==null)return null;
    if(!Number.isFinite(end)||end-this.armed<.04)return null;
    const item={index:this.index,start:this.armed,end};
    this.history.push(item);this.index++;this.cancel();return item;
  }
  down(time,repeat=false,wall=time*1000){
    if(repeat||this.pressing||this.done||!Number.isFinite(time)||time<0)return null;
    const committed=this.commit(time);
    if(this.armed!=null)return null;
    if(!this.done){this.armed=time;this.pressing=true;this.pressedAt=wall;}
    return committed;
  }
  up(time,wall=time*1000){
    if(!this.pressing)return null;
    this.pressing=false;
    // Use elapsed real time, so slow playback does not turn a hold into a tap.
    if(wall-this.pressedAt<180)return null;
    const item=this.commit(time);this.cancel();return item;
  }
  finish(time){return this.commit(time);}
  back(){this.cancel();const item=this.history.pop();if(item)this.index=item.index;return item;}
  get done(){return this.index>=this.count;}
}
function tapPreviewTarget(clock,committed,index,time,expected=.4){
  if(committed.has(index))return 1;
  if(index!==clock.index||clock.armed==null)return 0;
  return Math.max(0,Math.min(.96,1-Math.exp(-(time-clock.armed)/Math.max(.08,expected*.45))));
}
function smoothTapProgress(previous,target,seconds){
  return previous+(target-previous)*(1-Math.exp(-Math.max(0,seconds)/.055));
}
if(typeof module!=='undefined')module.exports={TapTiming,tapPreviewTarget,smoothTapProgress};
