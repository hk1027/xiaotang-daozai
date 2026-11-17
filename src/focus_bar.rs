//! 常驻岛的番茄水位、休息茶杯与倒计时。
use crate::{app::{Phase,Pomo},paint::{self,Card,H},theme::{self,W}};
use eframe::egui::{self,Color32};

pub(crate) const WIDTH:f32=68.0;
pub(crate) const LYRIC_WIDTH:f32=60.0;

fn tomato_outline(x:f32,y:f32)->Vec<(f32,f32)>{
    let curves:[[(f32,f32);4];4]=[
        [(0.0,-7.8),(-5.0,-10.4),(-11.0,-8.4),(-11.0,-0.4)],
        [(-11.0,-0.4),(-11.0,5.8),(-5.5,10.0),(0.0,9.2)],
        [(0.0,9.2),(5.5,10.0),(11.0,5.8),(11.0,-0.4)],
        [(11.0,-0.4),(11.0,-8.4),(5.0,-10.4),(0.0,-7.8)]
    ];
    let mut points=Vec::new();
    for curve in curves {for i in 0..16 {
        let t=i as f32/16.0;let a=1.0-t;
        let p=|axis:usize|{let v=|n:usize|if axis==0{curve[n].0}else{curve[n].1};a*a*a*v(0)+3.0*a*a*t*v(1)+3.0*a*t*t*v(2)+t*t*t*v(3)};
        points.push((x+p(0)*0.95,y+p(1)));
    }}
    points.push(points[0]);points.reverse();points
}

fn fill_polygon(c:&Card,points:&[(f32,f32)],ink:Color32){
    if points.len()<3 {return;}
    let centre=(points.iter().map(|p|p.0).sum::<f32>()/points.len() as f32,points.iter().map(|p|p.1).sum::<f32>()/points.len() as f32);
    let mut mesh=egui::Mesh::default();mesh.colored_vertex(c.pos(centre.0,centre.1),ink);
    for p in points {mesh.colored_vertex(c.pos(p.0,p.1),ink);}
    for i in 0..points.len() {mesh.add_triangle(0,i as u32+1,((i+1)%points.len()) as u32+1);}
    c.painter.add(egui::Shape::mesh(mesh));
}

fn liquid(points:&[(f32,f32)],fraction:f32,t:f32)->Vec<(f32,f32)>{
    if fraction<=0.0 {return vec![];}
    if fraction>=1.0 {return points.to_vec();}
    let min=points.iter().map(|p|p.1).fold(f32::INFINITY,f32::min);
    let max=points.iter().map(|p|p.1).fold(f32::NEG_INFINITY,f32::max);
    let level=max-(max-min)*fraction;let mut clipped=Vec::new();
    let amplitude=0.24*crate::anim::smoothstep(0.0,0.1,fraction)*(1.0-crate::anim::smoothstep(0.9,1.0,fraction));
    for pair in points.windows(2) {
        let (a,b)=(pair[0],pair[1]);
        if a.1>=level {clipped.push(a);}
        if (a.1>=level)!=(b.1>=level) {let f=(level-a.1)/(b.1-a.1);clipped.push((a.0+(b.0-a.0)*f,level));}
    }
    let mut wave=Vec::new();
    for i in 0..clipped.len() {
        let a=clipped[i];let b=clipped[(i+1)%clipped.len()];wave.push(a);
        if (a.1-level).abs()<0.001 && (b.1-level).abs()<0.001 && (a.0-b.0).abs()>1.0 {
            for n in 1..12 {let s=n as f32/12.0;wave.push((a.0+(b.0-a.0)*s,level+(t*1.2+s*5.5).sin()*(s*std::f32::consts::PI).sin()*amplitude));}
        }
    }
    wave
}

fn tomato(c:&Card,x:f32,y:f32,fraction:f32,t:f32){
    let c=c.scaled(x,y,0.82);
    let outline=tomato_outline(x,y+0.7);let ink=theme::RED_TOP;
    fill_polygon(&c,&outline,paint::mix(theme::CARD,ink,0.11));
    fill_polygon(&c,&liquid(&outline,fraction,t),ink);
    c.path(&outline,1.2,paint::mix(ink,theme::WHITE,0.08));
    for (dx,dy) in [(-3.3,2.0),(3.0,1.5),(0.3,5.8)] {
        let py=y+0.7+dy;
        if py>y+9.9-18.1*fraction+0.5 {c.seg((x+dx-0.5,py-0.65),(x+dx+0.5,py+0.65),0.55,paint::alpha(theme::WHITE,0.42));}
    }
    let y=y-7.1;let sway=(t*0.85).sin()*0.4;let leaf=theme::SOFT_GREEN;
    c.seg((x,y),(x+0.8+sway,y-3.6),1.0,leaf);
    c.poly(&[(x,y+0.5),(x-5.0,y-2.4+sway),(x-2.8,y+1.7)],leaf);
    c.poly(&[(x,y+0.5),(x+5.2,y-2.1-sway),(x+2.6,y+1.8)],leaf);
    c.poly(&[(x,y),(x+0.3+sway,y-3.3),(x+2.0,y-0.4)],leaf);
}

pub(crate) fn tea(c:&Card,x:f32,y:f32,fraction:f32,t:f32){
    let porcelain=Color32::from_rgb(245,242,234);
    c.fill(x-5.6,y-1.5,10.0,7.5,2.7,porcelain);
    c.fill(x-4.4,y-1.0,7.6,2.0,1.0,paint::mix(theme::CARD,Color32::from_rgb(225,170,83),fraction));
    c.stroke(x-5.6,y-1.5,10.0,7.5,2.7,0.8,theme::WHITE);
    c.arc(x+4.8,y+1.1,2.7,-std::f32::consts::FRAC_PI_2,std::f32::consts::FRAC_PI_2,1.1,porcelain);
    c.seg((x-6.5,y+7.3),(x+6.5,y+7.3),0.8,porcelain);
    for dx in [-2.8,1.0] {
        let lift=(t*0.85+dx).sin()*0.45;
        c.path(&[(x+dx,y-3.5),(x+dx+0.7,y-5.2-lift),(x+dx,y-7.5-lift)],0.75,paint::alpha(theme::WHITE,0.65));
    }
}

pub(crate) fn draw(c:&Card,cx:f32,cy:f32,t:f64,pomo:&Pomo,total:f32,compact:bool){
    let width=if compact{LYRIC_WIDTH}else{WIDTH};
    let left=cx-width/2.0;
    let t=if pomo.running&&crate::bar_layout::animations_enabled(){t as f32}else{0.0};
    let fraction=(pomo.remaining/total).clamp(0.0,1.0);
    let ink=if pomo.phase==Phase::Focus {
        tomato(c,left+12.0,cy,fraction,t);theme::RED_TOP
    }else{
        tea(c,left+12.0,cy,fraction,t);theme::SOFT_GREEN
    };
    let secs=pomo.remaining.max(0.0).ceil() as u32;
    c.text(left+width-20.0,cy+4.8,&format!("{:02}:{:02}",secs/60,secs%60),if compact{10.0}else{11.0},W::Bold,ink,H::Center);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn liquid_remains_bounded_when_full_nearly_empty_or_empty(){
        let outline=tomato_outline(0.0,0.0);
        let min=outline.iter().map(|p|p.1).fold(f32::INFINITY,f32::min);
        let max=outline.iter().map(|p|p.1).fold(f32::NEG_INFINITY,f32::max);
        for fraction in [0.0,0.001,0.01,0.1,0.5,0.9,0.99,0.999,1.0] {for t in [0.0,1.0,3.5,7.0] {
            let points=liquid(&outline,fraction,t);
            assert!(points.iter().all(|p|p.0.is_finite() && p.1.is_finite() && p.1>=min-0.001 && p.1<=max+0.001),"液面不能越过番茄边界");
            if fraction==0.0 {assert!(points.is_empty());}
            if fraction==1.0 {assert_eq!(points,outline);}
        }}
    }
    #[test]
    fn remaining_energy_area_decreases_with_time(){
        let outline=tomato_outline(0.0,0.0);
        let area=|points:Vec<(f32,f32)>|points.iter().zip(points.iter().cycle().skip(1)).take(points.len()).map(|(a,b)|a.0*b.1-b.0*a.1).sum::<f32>().abs()/2.0;
        let values:Vec<_>=[0.1,0.5,0.9].into_iter().map(|fraction|area(liquid(&outline,fraction,1.0))).collect();
        assert!(values.windows(2).all(|p|p[0]<p[1]),"剩余时间越少，番茄内的能量应越少");
    }
}

