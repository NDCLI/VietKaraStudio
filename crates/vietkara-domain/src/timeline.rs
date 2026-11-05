//! Beat-clock resize parity with static/timeline_edit.js.
use crate::{Project,edit::Command,timing::map_time};
#[derive(Debug,Clone,Copy)]
pub enum Edge {Start,End}
pub fn resize(project:&Project,index:usize,edge:Edge,delta:f64,duration:f64)->Result<Command,String> {
    project.validate().map_err(|e|e.to_string())?;
    if !delta.is_finite() || !duration.is_finite() || duration<=0. {return Err("Thời gian timeline không hợp lệ".into());}
    let word=project.words.get(index).ok_or("Không tìm thấy từ")?;
    if word.locked==Some(true) {return Err("Từ đang khóa".into());}
    let beat=|time|map_time(time,&project.anchors,false);
    let reference=|time|map_time(time,&project.anchors,true);
    let clamp=|value:f64,low:f64,high:f64|low.max(value.min(high));
    let mut current=word.clone();
    let mut commands=Vec::new();
    match edge {
        Edge::Start=>{
            let previous=index.checked_sub(1).and_then(|i|project.words.get(i));
            let join=project.settings.timeline_linked && previous.is_some_and(|previous|previous.line==word.line && (beat(word.start)-beat(previous.end)).abs()<=0.06);
            let low=if join {beat(previous.unwrap().start)+0.05}else{0.};
            let start=clamp(beat(word.start)+delta,low.max(0.),beat(word.end)-0.05);
            current.start=reference(start);
            if join {
                let mut previous=previous.unwrap().clone();
                if previous.locked==Some(true) {return Err("Từ liền trước đang khóa".into());}
                previous.end=current.start;previous.manual=Some(true);previous.estimated=false;
                commands.push(Command::ReplaceWord{id:previous.id.clone(),word:Box::new(previous)});
            }
        }
        Edge::End=>{
            let next=project.words.get(index+1);
            let join=project.settings.timeline_linked && next.is_some_and(|next|next.line==word.line && (beat(next.start)-beat(word.end)).abs()<=0.06);
            let high=if join {duration.min(beat(next.unwrap().end)-0.05)}else{duration};
            let end=clamp(beat(word.end)+delta,beat(word.start)+0.05,high);
            current.end=reference(end);
            if join {
                let mut next=next.unwrap().clone();
                if next.locked==Some(true) {return Err("Từ liền sau đang khóa".into());}
                next.start=current.end;next.manual=Some(true);next.estimated=false;
                commands.push(Command::ReplaceWord{id:next.id.clone(),word:Box::new(next)});
            }
        }
    }
    current.manual=Some(true);current.estimated=false;
    commands.push(Command::ReplaceWord{id:current.id.clone(),word:Box::new(current)});
    Ok(Command::Batch(commands))
}
