//! 岛仔的原生状态、矢量表情与表情图集播放。
use crate::anim::{approach, lerp, smoothstep};
use crate::paint::{Card, H};
use crate::theme::{self, W};
use crate::pet_expressions::Expression;
use eframe::egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PetState {
    Awake,
    Curious,
    Waking,
    Happy,
    Satisfied,
    Music,
    Worried,
    Focused,
    Dozing,
    Sleeping,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listening_uses_all_twelve_new_expressions_between_singing_poses(){
        let mut pet=Pet::default();let mut seen=std::collections::HashSet::new();
        for frame in 0..15000 {
            pet.step(PetState::Music,frame as f64/60.0,1.0/60.0,[0.0;2],true);
            if let Some(expression)=pet.expressions.current(){seen.insert(expression as usize);}
        }
        assert_eq!(seen.len(),12,"连续听歌应能轮换到十二组新表情");
    }
    #[test]
    fn listening_resumes_the_same_place_after_a_notice_or_pause(){
        let mut pet=Pet::default();
        for frame in 0..1080 {pet.step(PetState::Music,frame as f64/60.0,1.0/60.0,[0.0;2],true);}
        let expression=pet.expressions.current().expect("演唱一段后应穿插新表情");
        pet.step(PetState::Music,30.0,20.0,[0.0;2],false);
        assert_eq!(pet.expressions.current(),Some(expression),"展开面板应暂停轮换");
        for state in [PetState::Awake,PetState::Focused,PetState::Worried] {
            pet.step(state,31.0,1.0,[0.0;2],true);assert!(pet.expressions.current().is_none());
            pet.step(PetState::Music,42.0,0.0,[0.0;2],true);
            assert_eq!(pet.expressions.current(),Some(expression),"恢复听歌应从暂停处继续");
        }
        pet.react(PetState::Satisfied);pet.step(PetState::Satisfied,43.0,0.1,[0.0;2],true);
        assert_eq!(pet.expressions.current(),Some(crate::pet_expressions::Expression::Heart),"喝水反馈应保留优先级");
    }
    #[test]
    fn singing_keeps_each_expression_for_fifteen_seconds_after_the_transition() {
        let mut pet=Pet::default();
        pet.step(PetState::Music,0.0,0.0,[0.0;2],true);
        let pose=pet.music_pose();
        for frame in 1..960 {
            pet.step(PetState::Music,frame as f64/60.0,1.0/60.0,[0.0;2],true);
            assert_eq!(pet.music_pose(),pose,"一秒过渡和十五秒停留期间不能频繁换表情");
        }
        pet.step(PetState::Music,16.1,0.2,[0.0;2],true);
        assert_eq!(pet.expressions.current(),Some(Expression::Wink),"完整演唱一段后再穿插新表情");
        pet.step(PetState::Music,19.4,3.3,[0.0;2],true);
        assert_ne!(pet.music_pose(),pose);
    }
    #[test]
    fn singing_rotates_expressions_and_starts_with_the_microphone() {
        let mut pet=Pet::default();let mut poses=Vec::new();let mut eyes=Vec::new();
        let mut start=0.0;
        for expression in MUSIC_EXPRESSIONS.into_iter().take(7) {
            pet.music_age=start;
            for frame in 0..60 {pet.step(PetState::Music,frame as f64/60.0,1.0/60.0,[0.0;2],true);}
            poses.push(pet.music_pose());eyes.push(pet.eyes.map(|eye|(eye.height,eye.curve)));
            start+=MUSIC_PERIOD+expression.duration();
        }
        assert_eq!(poses,vec![MusicPose::Microphone,MusicPose::Hum,MusicPose::SideSing,MusicPose::Squint,MusicPose::HighNote,MusicPose::Smile,MusicPose::Microphone]);
        assert_ne!(eyes[0],eyes[2]);assert_ne!(eyes[2],eyes[3]);assert_ne!(eyes[3],eyes[4]);
        let age=pet.age;
        let music_age=pet.music_age;
        pet.step(PetState::Music,100.0,20.0,[0.0;2],false);
        assert_eq!(pet.age,age,"展开面板时不应跳过演唱表情");
        assert_eq!(pet.music_age,music_age);
        pet.step(PetState::Awake,101.0,0.1,[0.0;2],true);
        pet.step(PetState::Music,102.0,0.1,[0.0;2],true);
        assert!((pet.music_age-music_age-0.1).abs()<0.001,"恢复听歌后继续轮换");
    }
    #[test]
    fn microphone_moves_gently_while_the_head_keeps_facing_the_mouth() {
        let mut heights=Vec::new();let mut angles=Vec::new();
        for frame in 0..600 {
            let (head,angle)=microphone_pose(frame as f32/60.0);
            let tip=[head[0]+7.0*angle.cos(),head[1]+7.0*angle.sin()];
            let distance=|p:[f32;2]|((p[0]+0.8).powi(2)+(p[1]-20.7).powi(2)).sqrt();
            assert!(distance(head)<distance(tip),"麦头应在嘴边，麦柄朝外");
            assert!(head[0]-2.6>1.4 && head[1]-2.6>17.0,"不能遮住嘴或眼睛");
            assert!(tip[0]+1.0<16.0 && tip[1]+1.0<28.0,"摆动不能超出常驻槽位");
            heights.push(head[1]);angles.push(angle);
        }
        let range=|values:Vec<f32>|values.iter().copied().fold(f32::NEG_INFINITY,f32::max)-values.iter().copied().fold(f32::INFINITY,f32::min);
        assert!((0.6..1.1).contains(&range(heights)),"麦头应有细微起伏");
        assert!((0.10..0.18).contains(&range(angles)),"麦柄应有轻微转动");
    }
    #[test]
    #[ignore="按需导出听歌状态的实际界面动图"]
    fn listening_expression_gallery(){
        use eframe::egui::{self,Pos2,Rect,Vec2};
        use crate::app::{App,Env,Options,WIN_W,WIN_H};
        let dest=std::path::PathBuf::from(std::env::var_os("DAOZAI_LISTENING_PREVIEW").expect("需指定预览目录"));
        std::fs::create_dir_all(&dest).unwrap();
        std::env::set_var("DAOZAI_DATA_DIR",std::env::temp_dir().join(format!("daozai-listening-{}",std::process::id())));
        let ctx=egui::Context::default();let mut app=App::new(&ctx,Options{fetch_lyrics:false,pet_idle:1e9});
        app.settings.pet=true;app.settings.minimal=false;app.settings.island_lyrics=false;
        let mut scenes=Vec::new();let mut start=0.0;
        for (index,expression) in MUSIC_EXPRESSIONS.into_iter().enumerate() {
            if [0,2].contains(&index) {scenes.push((format!("演唱动作 {}",index+1),start));}
            if [0,1,3,4,5,11].contains(&index) {
                let name=match expression {Expression::Wink=>"调皮眨眼",Expression::Delighted=>"偷偷开心",Expression::Blush=>"害羞脸红",
                    Expression::Heart=>"悄悄比心",Expression::Scan=>"慢慢巡视",_=>"左右张望"};
                scenes.push((name.to_owned(),start+MUSIC_PERIOD));
            }
            start+=MUSIC_PERIOD+expression.duration();
        }
        let mut raster=crate::preview_raster::Raster::new(1280,540);let mut sequence=0;
        for (index,(_,at)) in scenes.iter().enumerate() {
            app.pet.music_age=*at;
            for frame in 0..32 {
                let t=sequence as f64/10.0;sequence+=1;
                let env=Env{t,clock:"09:41:00".into(),today:"2026-11-07".into(),date:"11月7日 周六".into(),idle_seconds:Some(0.0),
                    media:crate::smtc::NowPlaying{title:"晚风与星光".into(),artist:"示例歌手".into(),playing:true,duration:240.0,..Default::default()},..Default::default()};
                let input=egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(WIN_W,WIN_H))),time:Some(t),
                    viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(2.0),..Default::default()})).collect(),..Default::default()};
                let output=ctx.run(input,|ctx|{egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{app.frame(ui,ui.max_rect(),&env);});});
                raster.update_textures(&output.textures_delta);raster.buf.fill([0.95,0.96,0.98,1.0]);
                raster.draw(&ctx.tessellate(output.shapes,output.pixels_per_point),output.pixels_per_point);
                let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);let image=image::RgbaImage::from_raw(1280,540,rgba).unwrap();
                image::imageops::crop_imm(&image,420,0,440,100).to_image().save(dest.join(format!("music-{index:02}-{frame:03}.png"))).unwrap();
                raster.free_textures(&output.textures_delta);
            }
        }
        std::fs::write(dest.join("scenes.json"),serde_json::to_string(&scenes).unwrap()).unwrap();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MusicPose { Microphone, Hum, SideSing, Squint, HighNote, Smile }

const MUSIC_PERIOD:f32=16.0;
const MUSIC_EXPRESSIONS:[Expression;12]=[
    Expression::Wink,Expression::Delighted,Expression::Smug,Expression::Blush,
    Expression::Heart,Expression::Scan,Expression::SideGlance,Expression::Giggle,
    Expression::Praised,Expression::Peek,Expression::ShyPeek,Expression::Search,
];
struct MusicSegment {
    pose:MusicPose,
    elapsed:f32,
    expression:Option<(Expression,f32)>,
}
fn music_segment_at(age:f32)->MusicSegment {
    let cycle=MUSIC_EXPRESSIONS.iter().map(|expression|MUSIC_PERIOD+expression.duration()).sum::<f32>();
    let mut time=age%cycle;
    for (index,expression) in MUSIC_EXPRESSIONS.into_iter().enumerate() {
        let pose=match index%6 {0=>MusicPose::Microphone,1=>MusicPose::Hum,2=>MusicPose::SideSing,3=>MusicPose::Squint,4=>MusicPose::HighNote,_=>MusicPose::Smile};
        if time<MUSIC_PERIOD {return MusicSegment{pose,elapsed:time,expression:None};}
        time-=MUSIC_PERIOD;
        if time<expression.duration(){return MusicSegment{pose,elapsed:MUSIC_PERIOD,expression:Some((expression,time))};}
        time-=expression.duration();
    }
    MusicSegment{pose:MusicPose::Microphone,elapsed:0.0,expression:None}
}

fn microphone_pose(age:f32)->([f32;2],f32) {
    ([5.9+(age*0.95).sin()*0.2,20.8+(age*1.35).sin()*0.45],35.0_f32.to_radians()+(age*1.1).sin()*0.07)
}

fn microphone(c:&Card,cx:f32,y:f32,age:f32){
    let age=if crate::bar_layout::animations_enabled(){age}else{0.0};
    let (head,angle)=microphone_pose(age);
    let point=|s:f32,v:f32|(cx+head[0]+s*angle.cos()-v*angle.sin(),y+head[1]+s*angle.sin()+v*angle.cos());
    let capsule=|start:f32,end:f32,width:f32,color:Color32|{
        let a=point(start,0.0);let b=point(end,0.0);
        c.seg(a,b,width,color);c.circle(a.0,a.1,width/2.0,color);c.circle(b.0,b.1,width/2.0,color);
    };
    let line=|a:(f32,f32),b:(f32,f32),width:f32,color:Color32|c.seg(point(a.0,a.1),point(b.0,b.1),width,color);
    capsule(2.3,7.0,1.9,Color32::from_rgb(176,183,197));
    capsule(2.5,6.8,1.45,Color32::from_rgb(76,84,101));
    line((3.4,-0.53),(6.4,-0.53),0.28,Color32::from_rgb(218,225,237));
    capsule(2.05,2.65,2.6,Color32::from_rgb(126,137,157));
    let head=point(0.0,0.0);
    c.circle(head.0,head.1,2.6,Color32::from_rgb(222,228,238));
    c.circle(head.0,head.1,2.15,Color32::from_rgb(164,176,194));
    for s in [-1.0f32,0.0,1.0] {let v=(4.0-s*s).sqrt()*0.88;line((s,-v),(s,v),0.32,Color32::from_rgb(92,105,126));}
    for v in [-1.0f32,0.0,1.0] {let s=(4.0-v*v).sqrt()*0.88;line((-s,v),(s,v),0.3,Color32::from_rgb(226,232,243));}
}

#[derive(Clone, Copy)]
struct Eye {
    height: f32,
    spread: f32,
    curve: f32,
    y: f32,
    tilt: f32,
}

impl Eye {
    fn open(height: f32) -> Self { Self { height, spread: 0.0, curve: 0.0, y: 0.0, tilt: 0.0 } }
    fn arc(spread: f32, curve: f32) -> Self { Self { spread, curve, ..Self::open(0.0) } }
    fn follow(&mut self, goal: Self, dt: f32) {
        self.height = approach(self.height, goal.height, 17.0, dt);
        self.spread = approach(self.spread, goal.spread, 17.0, dt);
        self.curve = approach(self.curve, goal.curve, 17.0, dt);
        self.y = approach(self.y, goal.y, 12.0, dt);
        self.tilt = approach(self.tilt, goal.tilt, 12.0, dt);
    }
}

pub(crate) struct Pet {
    pub state: PetState,
    pub reaction: Option<(PetState, f32)>,
    pub waking_until: f64,
    pub last_idle: f64,
    pub hovered: bool,
    pub worried: bool,
    pub expressions: crate::pet_expressions::Expressions,
    age: f32,
    music_age:f32,
    eyes: [Eye; 2],
    gaze: [f32; 2],
    y: f32,
}

impl Default for Pet {
    fn default() -> Self {
        Self { state: PetState::Awake, reaction: None, waking_until: 0.0, last_idle: 0.0,
            hovered: false, worried: false, expressions:Default::default(), age: 0.0, music_age:0.0, eyes: [Eye::open(4.4); 2], gaze: [0.0; 2], y: 0.0 }
    }
}

impl Pet {
    pub fn react(&mut self, state: PetState) { self.reaction = Some((state, self.expressions.react(state))); self.age = 0.0; }
    /// 反馈只在实际可见时计时，避免用户还在面板里操作时就播完。
    pub fn advance(&mut self, dt: f32, visible: bool) {
        if visible {
            if let Some((_, remaining)) = &mut self.reaction {
                *remaining -= dt;
                if *remaining <= 0.0 { self.reaction = None; }
            }
        }
    }
    pub fn music_pose(&self)->MusicPose {
        music_segment_at(self.music_age).pose
    }

    pub fn step(&mut self, state: PetState, t: f64, dt: f32, gaze: [f32; 2], visible: bool) {
        if self.state==PetState::Music && matches!(state,PetState::Awake|PetState::Curious) {self.expressions.clear();}
        self.expressions.step(state,dt,visible);
        if state != self.state { self.state = state; self.age = 0.0; }
        if visible { self.age += dt; }
        if state==PetState::Music && visible {
            self.music_age+=dt;
            if let Some((expression,time))=music_segment_at(self.music_age).expression {self.expressions.play_at(expression,time);}
        }
        let age = self.age;
        let mut eyes = [Eye::open(4.4); 2];
        let mut y = (t as f32 * 1.7).sin() * 0.35;
        let mut look = gaze;
        match state {
            PetState::Awake => {},
            PetState::Curious => {
                eyes[0].height = 5.6; eyes[1].height = 6.3;
                eyes[0].y = 0.6; eyes[1].y = -0.6;
            },
            PetState::Waking => {
                eyes[0].height = lerp(0.0, 4.4, smoothstep(0.1, 0.65, age));
                eyes[1].height = lerp(0.0, 4.4, smoothstep(0.4, 1.1, age));
                y = 1.0 - smoothstep(0.0, 1.2, age); look = [0.0; 2];
            },
            PetState::Happy => {
                eyes = [Eye::arc(2.6, -1.8); 2];
                y = -(age * 7.5).sin().abs() * 2.0 * (-age * 0.65).exp();
            },
            PetState::Satisfied => {
                eyes = [Eye::arc(2.2, -1.2); 2];
                y = (age * 8.0).sin() * 1.2 * (-age * 0.8).exp();
            },
            PetState::Music => {
                let age=self.music_age;
                let sway=(age*1.4).sin();
                look=[0.0,0.0];y=-0.5+sway*0.25;
                match self.music_pose() {
                    MusicPose::Microphone=>{eyes=[Eye::arc(2.5,0.7);2];},
                    MusicPose::SideSing=>{eyes=[Eye::open(1.6);2];eyes[0].y=-0.5;eyes[1].y=0.5;look=[0.4,0.0];y=sway*0.35;},
                    MusicPose::HighNote=>{eyes=[Eye::arc(2.3,-0.7);2];eyes[0].tilt=-0.3;eyes[1].tilt=0.3;y=-1.5+sway*0.2;},
                    MusicPose::Hum=>{eyes=[Eye::arc(2.5,0.7);2];y=sway*0.3;},
                    MusicPose::Squint=>{eyes=[Eye::arc(1.6,0.0);2];y=sway*0.45;},
                    MusicPose::Smile=>{eyes=[Eye::arc(2.3,-0.8);2];eyes[0].y=-0.5;eyes[1].y=0.5;look=[sway*0.5,0.0];},
                }
            },
            PetState::Worried => {
                eyes[0] = Eye { spread: -0.6, tilt: -0.7, y: 0.4, ..Eye::open(2.8) };
                eyes[1] = Eye { spread: 0.6, tilt: 0.7, y: 1.0, ..Eye::open(1.8) };
            },
            PetState::Focused => { eyes = [Eye::open(1.8); 2]; look = [gaze[0] * 0.2, 0.0]; y = 0.0; },
            PetState::Dozing => { eyes = [Eye::open(0.6); 2]; y = 1.0; look = [0.0; 2]; },
            PetState::Sleeping => { eyes = [Eye::arc(2.4, 1.0); 2]; y = 0.8 + (t as f32 * 1.2).sin() * 0.4; look = [0.0; 2]; },
        }
        if self.expressions.current().is_some() {eyes=[Eye::open(4.4);2];look=[0.0;2];y=0.0;}
        if self.expressions.current().is_none() && matches!(state, PetState::Awake | PetState::Curious | PetState::Focused | PetState::Worried) {
            let start = 2.0 + crate::anim::hash01((t / 5.6).floor() as u64) * 2.7;
            let blink = ((t % 5.6) as f32 - start) / 0.18;
            if (0.0..1.0).contains(&blink) {
                for eye in &mut eyes { eye.height *= 1.0 - (blink * std::f32::consts::PI).sin(); }
            }
        }
        let eye_dt=if state==PetState::Music {dt*0.2}else{dt};
        for (eye, goal) in self.eyes.iter_mut().zip(eyes) { eye.follow(goal, eye_dt); }
        for (axis, goal) in self.gaze.iter_mut().zip(look) { *axis = approach(*axis, goal, 10.0, dt); }
        self.y = approach(self.y, y, 14.0, dt);
    }

    pub fn draw(&mut self, c: &Card, cx: f32, t: f64, palette:Option<crate::skin_paint::Palette>) {
        if self.expressions.draw(c,cx,palette) {return;}
        for (i, eye) in self.eyes.iter().enumerate() {
            let x = cx + if i == 0 { -5.6 } else { 5.6 } + self.gaze[0];
            let y = 14.5 + self.y + eye.y + self.gaze[1];
            let points: Vec<_> = (0..=12).map(|n| {
                let q = n as f32 / 6.0 - 1.0;
                (x + q * eye.spread, y + q * (eye.height / 2.0 + eye.tilt) + (1.0 - q*q) * eye.curve)
            }).collect();
            let stroke=if self.state==PetState::Music {3.6-0.8*smoothstep(0.5,1.5,eye.spread.abs())}else{3.6};
            c.path(&points, stroke, theme::WHITE);
            // 圆端点保持胶囊眼睛的轮廓。
            for p in [points[0], points[12]] { c.circle(p.0, p.1, stroke/2.0, theme::WHITE); }
        }
        match self.state {
            PetState::Dozing | PetState::Sleeping => {
                let bob = ((t as f32 * 1.4).sin() + 1.0) * 0.7;
                c.text(cx+14.0, 13.0-bob, "z", 7.0, W::Bold, Color32::from_rgb(170,160,255), H::Left);
                if self.state == PetState::Sleeping { c.text(cx+19.0, 9.0-bob, "z", 5.5, W::Bold, Color32::from_rgb(170,160,255), H::Left); }
            },
            PetState::Happy => {
                let k = smoothstep(0.0, 0.2, self.age) * (1.0-smoothstep(1.5, 2.5, self.age));
                let c = c.faded(k);
                for (dx, y, r) in [(-18.0, 9.0, 2.1), (18.0, 8.0, 2.8)] {
                    c.seg((cx+dx-r,y),(cx+dx+r,y),1.0,theme::GREEN);
                    c.seg((cx+dx,y-r),(cx+dx,y+r),1.0,theme::GREEN);
                }
            },
            PetState::Satisfied => crate::paint::droplet(c,cx+18.0,12.0,5.0,theme::CYAN),
            PetState::Music => {
                let segment=music_segment_at(self.music_age);
                self.draw_music(&c.faded(smoothstep(0.0,1.0,segment.elapsed)),cx,segment.pose);
            },
            PetState::Worried => {
                c.fill(cx+17.0,8.0,1.6,4.2,0.8,theme::YELLOW);
                c.circle(cx+17.8,14.5,0.9,theme::YELLOW);
            },
            _ => {},
        }
    }

    fn draw_music(&self,c:&Card,cx:f32,pose:MusicPose){
        let y=self.y+self.gaze[1];
        let pulse=(self.music_age*2.0).sin()*0.5+0.5;
        let mouth=|rx:f32,ry:f32|{
            let points:Vec<_>=(0..=24).map(|n|{let angle=n as f32/24.0*std::f32::consts::TAU;(cx-0.8+angle.cos()*rx,20.7+y+angle.sin()*ry)}).collect();
            c.path(&points,0.9,theme::WHITE);
        };
        match pose {
            MusicPose::Microphone=>mouth(1.1,0.8+pulse*0.5),
            MusicPose::SideSing=>mouth(1.5,0.8+pulse*0.3),
            MusicPose::HighNote=>mouth(1.7,1.6+pulse*0.5),
            MusicPose::Hum=>c.path(&[(cx-2.7,20.7+y),(cx-1.8,21.4+y),(cx-0.8,21.0+y),(cx+0.2,21.4+y),(cx+1.1,20.7+y)],0.9,theme::WHITE),
            MusicPose::Squint=>mouth(1.2,0.4+pulse*0.6),
            MusicPose::Smile=>c.path(&[(cx-2.5,20.6+y),(cx-1.6,21.6+y),(cx-0.5,21.9+y),(cx+0.6,21.6+y),(cx+1.5,20.6+y)],1.0,theme::WHITE),
        }
        if matches!(pose,MusicPose::Microphone|MusicPose::SideSing|MusicPose::HighNote) {
            microphone(c,cx,y,self.music_age);
        } else {
            let y=8.5+(self.music_age*1.4).sin()*0.6;
            c.seg((cx+16.0,y-3.0),(cx+16.0,y+3.0),1.1,theme::WHITE);
            c.seg((cx+16.0,y-3.0),(cx+19.0,y-2.0),1.1,theme::WHITE);
            c.circle(cx+14.8,y+3.0,1.5,theme::WHITE);
        }
    }
}
