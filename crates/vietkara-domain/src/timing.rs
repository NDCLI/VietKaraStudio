//! Shared piecewise reference/beat mapping, including endpoint extrapolation.
use crate::Anchor;
pub fn map_time(t:f64, anchors:&[Anchor], inverse:bool)->f64 {
    let coordinates=|a:&Anchor|if inverse {(a.beat,a.r#ref)}else{(a.r#ref,a.beat)};
    if anchors.is_empty() {return t;}
    if anchors.len()==1 {let (x,y)=coordinates(&anchors[0]);return t+y-x;}
    let pair=anchors.windows(2).find(|pair|t<=coordinates(&pair[1]).0).unwrap_or(&anchors[anchors.len()-2..]);
    let (a,b)=coordinates(&pair[0]);let (c,d)=coordinates(&pair[1]);
    b+(t-a)*(d-b)/(c-a)
}
