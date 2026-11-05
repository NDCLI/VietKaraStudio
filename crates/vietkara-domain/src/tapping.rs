//! Beat-clock tapping state machine, ported from static/tap_timing.js.
//! `wall_ms` is monotonic wall time, distinct from playback time.
#[derive(Debug,Clone,Copy,PartialEq)]
pub struct Tap {pub index:usize,pub start:f64,pub end:f64}
#[derive(Debug,Clone)]
pub struct TapTiming {
    count:usize, pub index:usize, pub history:Vec<Tap>,
    pub armed:Option<f64>, pub pressing:bool, pressed_at:Option<f64>,
}
impl TapTiming {
    pub fn new(count:usize,index:usize)->Result<Self,String> {
        if count==0 {return Err("Cấu hình gõ nhịp không hợp lệ".into());}
        Ok(Self {count,index:index.min(count-1),history:Vec::new(),armed:None,pressing:false,pressed_at:None})
    }
    pub fn done(&self)->bool {self.index>=self.count}
    pub fn cancel(&mut self) {self.armed=None;self.pressing=false;self.pressed_at=None;}
    fn commit(&mut self,end:f64)->Option<Tap> {
        let start=self.armed?;
        if !end.is_finite() || end-start<0.04 {return None;}
        let tap=Tap {index:self.index,start,end};
        self.history.push(tap);self.index+=1;self.cancel();Some(tap)
    }
    pub fn down(&mut self,time:f64,repeat:bool,wall_ms:f64)->Option<Tap> {
        if repeat || self.pressing || self.done() || !time.is_finite() || time<0. || !wall_ms.is_finite() {return None;}
        let committed=self.commit(time);
        if self.armed.is_some() {return None;}
        if !self.done() {self.armed=Some(time);self.pressing=true;self.pressed_at=Some(wall_ms);}
        committed
    }
    pub fn up(&mut self,time:f64,wall_ms:f64)->Option<Tap> {
        if !self.pressing || !wall_ms.is_finite() {return None;}
        self.pressing=false;
        if wall_ms-self.pressed_at.unwrap_or(wall_ms)<180. {return None;}
        let item=self.commit(time);self.cancel();item
    }
    pub fn finish(&mut self,time:f64)->Option<Tap> {self.commit(time)}
    pub fn back(&mut self)->Option<Tap> {
        self.cancel();let item=self.history.pop()?;self.index=item.index;Some(item)
    }
}

/// Build a validated reversible command without modifying the live project.
pub fn tap_command(project:&crate::Project,tap:Tap)->Result<crate::edit::Command,String> {
    let mut word=project.words.get(tap.index).cloned().ok_or("Không tìm thấy từ đang gõ nhịp")?;
    if word.locked==Some(true) {return Err("Từ đang khóa".into());}
    word.start=crate::timing::map_time(tap.start,&project.anchors,true);
    word.end=crate::timing::map_time(tap.end,&project.anchors,true);
    if !word.start.is_finite() || !word.end.is_finite() || word.start<0. || word.end<=word.start {
        return Err("Mốc đồng bộ đưa từ ra trước bản hát hoặc tạo thời lượng không hợp lệ".into());
    }
    word.manual=Some(true);word.estimated=false;
    Ok(crate::edit::Command::ReplaceWord {id:word.id.clone(),word:Box::new(word)})
}
