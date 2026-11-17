//! 岛仔表情：按场景播放，隐藏时暂停，日常动作之间留出安静时间。
//! 每个表情单独一张 144×1440 的序列图（16 帧 × 3 倍密度），只解码、上传正在播放的那一张，播完即释放。
use crate::pet::PetState;
use crate::paint::Card;
use crate::skin_paint::Palette;
use eframe::egui::{self, Color32, Rect, TextureHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Expression {
    Wink, Smug, Blush, Delighted, SideGlance, Praised,
    Heart, Giggle, ShyPeek, Scan, Peek, Search,
}

impl Expression {
    pub const ALL:[Self;12]=[Self::Wink,Self::Smug,Self::Blush,Self::Delighted,Self::SideGlance,Self::Praised,
        Self::Heart,Self::Giggle,Self::ShyPeek,Self::Scan,Self::Peek,Self::Search];
    fn times(self)->[f32;16] {
        if self as usize>=9 {[0.35,0.13,0.15,0.18,0.35,0.28,0.16,0.11,0.11,0.16,0.35,0.28,0.16,0.14,0.20,0.35]}
        else {[0.35,0.12,0.12,0.12,0.13,0.13,0.14,0.40,0.18,0.14,0.12,0.12,0.12,0.14,0.14,0.45]}
    }
    pub fn duration(self)->f32 {self.times().iter().sum()}
    /// 由 `assets/pet/expressions.png` 按列拆出，顺序与 `ALL` 一致。
    fn sheet(self)->&'static [u8] {
        const SHEETS:[&[u8];12]=[
            include_bytes!("../assets/pet/expressions/00-wink.png"),include_bytes!("../assets/pet/expressions/01-smug.png"),
            include_bytes!("../assets/pet/expressions/02-blush.png"),include_bytes!("../assets/pet/expressions/03-delighted.png"),
            include_bytes!("../assets/pet/expressions/04-side-glance.png"),include_bytes!("../assets/pet/expressions/05-praised.png"),
            include_bytes!("../assets/pet/expressions/06-heart.png"),include_bytes!("../assets/pet/expressions/07-giggle.png"),
            include_bytes!("../assets/pet/expressions/08-shy-peek.png"),include_bytes!("../assets/pet/expressions/09-scan.png"),
            include_bytes!("../assets/pet/expressions/10-peek.png"),include_bytes!("../assets/pet/expressions/11-search.png"),
        ];
        SHEETS[self as usize]
    }
    fn frame(self,time:f32)->usize {
        let mut end=0.0;
        for (index,seconds) in self.times().into_iter().enumerate() {end+=seconds;if time<end{return index;}}
        15
    }
}

pub(crate) struct Expressions {
    clip:Option<(Expression,f32)>,
    wait:f32,
    cooldown:f32,
    hovered:bool,
    next:[usize;4],
    /// 正在播放的表情序列图；换表情或换皮肤时原地替换内容，播完释放。
    sheet:Option<(Expression,Option<Palette>,TextureHandle)>,
}

impl Default for Expressions {
    fn default()->Self {Self{clip:None,wait:8.0,cooldown:0.0,hovered:false,next:[0;4],sheet:None}}
}

impl Expressions {
    pub fn current(&self)->Option<Expression> {self.clip.map(|clip|clip.0)}
    pub fn play(&mut self,expression:Expression) {self.clip=Some((expression,0.0));}
    pub fn play_at(&mut self,expression:Expression,time:f32) {self.clip=Some((expression,time));}
    pub fn clear(&mut self) {self.clip=None;self.wait=8.0;self.cooldown=0.0;self.hovered=false;self.sheet=None;}
    fn choose(&mut self,bucket:usize,pool:&[Expression])->Expression {
        let expression=pool[self.next[bucket]%pool.len()];self.next[bucket]=self.next[bucket].wrapping_add(1);expression
    }
    pub fn react(&mut self,state:PetState)->f32 {
        use Expression::*;
        let expression=match state {
            PetState::Happy=>self.choose(2,&[Praised,Smug,Delighted,Giggle]),
            PetState::Satisfied=>self.choose(3,&[Heart,Delighted]),
            _=>return 2.6,
        };
        self.play(expression);expression.duration()
    }
    pub fn step(&mut self,state:PetState,dt:f32,visible:bool) {
        use Expression::*;
        if !visible {return;}
        if !matches!(state,PetState::Awake|PetState::Curious|PetState::Happy|PetState::Satisfied) {self.clear();return;}
        self.cooldown=(self.cooldown-dt).max(0.0);
        if let Some((expression,time))=&mut self.clip {
            *time+=dt;
            if *time>=expression.duration() {self.clip=None;self.sheet=None;self.wait=22.0+(self.next[0]%4) as f32*3.0;}
        }
        let hovered=state==PetState::Curious;
        if hovered && !self.hovered && self.cooldown<=0.0 && self.clip.is_none() {
            let expression=self.choose(1,&[Wink,Blush,SideGlance,ShyPeek]);self.play(expression);self.cooldown=18.0;
        }
        self.hovered=hovered;
        if state==PetState::Awake && self.clip.is_none() && crate::bar_layout::animations_enabled() {
            self.wait-=dt;
            if self.wait<=0.0 {
                let expression=self.choose(0,&[Scan,Delighted,Peek,Smug,Search,Giggle,Praised,ShyPeek]);self.play(expression);
            }
        }
    }
    pub fn draw(&mut self,c:&Card,cx:f32,palette:Option<Palette>)->bool {
        let Some((expression,time))=self.clip else{return false;};
        if self.sheet.as_ref().is_none_or(|(previous,previous_palette,_)|(*previous,*previous_palette)!=(expression,palette)) {
            let decoded=image::load_from_memory(expression.sheet()).expect("内置表情序列图").into_rgba8();
            let mut image=egui::ColorImage::from_rgba_unmultiplied([decoded.width() as usize,decoded.height() as usize],decoded.as_raw());
            drop(decoded);
            if let Some(palette)=palette {for pixel in &mut image.pixels {if pixel.a()>0 {*pixel=crate::skin_paint::map_color(*pixel,&palette);}}}
            if let Some((previous,previous_palette,texture))=&mut self.sheet {*previous=expression;*previous_palette=palette;texture.set(image,egui::TextureOptions::LINEAR);}
            else {
                let texture=c.ui.ctx().load_texture("daozai-expressions",image,egui::TextureOptions::LINEAR);
                crate::skin_paint::mark_colored(texture.id());self.sheet=Some((expression,palette,texture));
            }
        }
        let frame=if crate::bar_layout::animations_enabled(){expression.frame(time)}else{7};
        let uv=Rect::from_min_max(egui::pos2(0.0,frame as f32/16.0),egui::pos2(1.0,(frame+1) as f32/16.0));
        c.painter.image(self.sheet.as_ref().unwrap().2.id(),c.rect(cx-24.0,0.0,48.0,30.0),uv,Color32::WHITE);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expressions_follow_real_scenarios_and_system_animation_setting(){
        let mut expressions=Expressions::default();let mut seen=std::collections::HashSet::new();
        let idle_animations=crate::bar_layout::animations_enabled();
        if idle_animations {
            for _ in 0..8 {
                for _ in 0..400 {
                    expressions.step(PetState::Awake,0.1,true);
                    if expressions.current().is_some(){break;}
                }
                seen.insert(expressions.current().expect("日常表情应在等待时间内触发") as usize);
                for _ in 0..100 {
                    expressions.step(PetState::Awake,0.1,true);
                    if expressions.current().is_none(){break;}
                }
                assert!(expressions.current().is_none(),"日常表情应按时结束");
                for _ in 0..210 {expressions.step(PetState::Awake,0.1,true);assert!(expressions.current().is_none(),"日常动作之间应有安静间隔");}
            }
        }else{
            for _ in 0..400 {
                expressions.step(PetState::Awake,0.1,true);
                assert!(expressions.current().is_none(),"系统关闭动画时应保持安静待机");
            }
        }
        for _ in 0..4 {
            expressions.clear();expressions.step(PetState::Curious,0.1,true);seen.insert(expressions.current().unwrap() as usize);
        }
        for state in [PetState::Happy,PetState::Happy,PetState::Happy,PetState::Happy,PetState::Satisfied,PetState::Satisfied] {
            expressions.react(state);seen.insert(expressions.current().unwrap() as usize);
        }
        let expected=Expression::ALL.into_iter().filter(|expression|idle_animations || !matches!(expression,Expression::Scan|Expression::Peek|Expression::Search))
            .map(|expression|expression as usize).collect::<std::collections::HashSet<_>>();
        assert_eq!(seen,expected,"应覆盖当前系统动画设置下可触发的全部表情");
    }
    #[test]
    fn hidden_feedback_pauses_and_hover_has_a_cooldown(){
        let mut expressions=Expressions::default();expressions.react(PetState::Happy);
        expressions.step(PetState::Happy,30.0,false);assert_eq!(expressions.clip.unwrap().1,0.0);
        expressions.step(PetState::Happy,1.0,true);assert_eq!(expressions.clip.unwrap().1,1.0);
        expressions.clear();expressions.step(PetState::Curious,0.1,true);let first=expressions.current().unwrap();
        for _ in 0..50 {expressions.step(PetState::Curious,0.1,true);}
        expressions.step(PetState::Awake,0.1,true);expressions.step(PetState::Curious,0.1,true);
        assert!(expressions.current().is_none(),"快速反复经过不能一直重播互动");
        for _ in 0..180 {expressions.step(PetState::Curious,0.1,true);}
        expressions.step(PetState::Awake,0.1,true);expressions.step(PetState::Curious,0.1,true);
        assert_ne!(expressions.current().unwrap(),first);
    }
    #[test]
    fn focus_music_sleep_and_warning_keep_priority(){
        let mut expressions=Expressions::default();
        for state in [PetState::Focused,PetState::Music,PetState::Worried,PetState::Waking,PetState::Dozing,PetState::Sleeping] {
            expressions.play(Expression::Wink);expressions.step(state,0.1,true);assert!(expressions.current().is_none());
        }
    }
    #[test]
    fn sheets_keep_alpha_and_the_looking_eyes_really_move(){
        for expression in Expression::ALL {
            let image=image::load_from_memory(expression.sheet()).unwrap().into_rgba8();
            assert_eq!(image.dimensions(),(144,1440),"每张序列图 16 帧、保持 3 倍密度");assert_eq!(image.get_pixel(0,0).0[3],0);
            let mut centers=Vec::new();
            for frame in 0..16 {
                let mut xs=Vec::new();
                for y in 0..90 {for x in 0..144 {if image.get_pixel(x,frame*90+y).0[3]>150 {xs.push(x as f32/3.0);}}}
                assert!(!xs.is_empty());centers.push(xs.iter().sum::<f32>()/xs.len() as f32);
            }
            if expression as usize>=9 {assert!(centers.iter().copied().fold(f32::NEG_INFINITY,f32::max)-centers.iter().copied().fold(f32::INFINITY,f32::min)>8.0);}
            assert_eq!(expression.frame(0.0),0);assert_eq!(expression.frame(expression.duration()),15);
        }
    }
    #[test]
    fn only_the_playing_sheet_is_uploaded_and_light_skins_keep_dark_eyes(){
        let ctx=egui::Context::default();let mut expressions=Expressions::default();expressions.play(Expression::Wink);
        let frame=|expressions:&mut Expressions,palette|ctx.run(Default::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let card=Card::new(ui,egui::Pos2::ZERO,ui.max_rect());assert!(expressions.draw(&card,24.0,palette));
            });
        });
        let output=frame(&mut expressions,None);let id=expressions.sheet.as_ref().unwrap().2.id();
        let delta=&output.textures_delta.set.iter().find(|(texture,_)|*texture==id).unwrap().1;
        let egui::ImageData::Color(image)=&delta.image else{panic!("表情应为彩色纹理")};
        assert_eq!(image.size,[144,1440],"只上传正在播放的表情，不上传整张图集");
        let white=image.pixels.iter().position(|p|p.a()>250&&p.r()>240&&p.g()>240&&p.b()>240).unwrap();
        assert!(!frame(&mut expressions,None).textures_delta.set.iter().any(|(texture,_)|*texture==id),"同一表情、同一皮肤不能重复解码或上传");
        for palette in [crate::skin_paint::CREAM,crate::skin_paint::GLASS_LIGHT] {
            let output=frame(&mut expressions,Some(palette));assert_eq!(expressions.sheet.as_ref().unwrap().2.id(),id);
            let delta=&output.textures_delta.set.iter().find(|(texture,_)|*texture==id).unwrap().1;
            let egui::ImageData::Color(image)=&delta.image else{unreachable!()};
            assert!(image.pixels[white].r()<70&&image.pixels[white].g()<70&&image.pixels[white].b()<70,"浅色皮肤上的眼睛应为深色");
        }
        expressions.play(Expression::Search);
        let output=frame(&mut expressions,None);assert_eq!(expressions.sheet.as_ref().unwrap().2.id(),id,"换表情原地替换，不新建纹理");
        assert!(output.textures_delta.set.iter().any(|(texture,_)|*texture==id));
        for _ in 0..100 {expressions.step(PetState::Awake,0.1,true);if expressions.current().is_none(){break;}}
        assert!(expressions.current().is_none() && expressions.sheet.is_none(),"播完释放纹理，不常驻显存");
        let output=ctx.run(Default::default(),|_|{});assert!(output.textures_delta.free.contains(&id));
    }

    #[test]
    fn split_sheets_match_the_packed_atlas_pixel_for_pixel(){
        let atlas=image::load_from_memory(include_bytes!("../assets/pet/expressions.png")).unwrap().into_rgba8();
        for expression in Expression::ALL {
            let sheet=image::load_from_memory(expression.sheet()).unwrap().into_rgba8();
            let column=image::imageops::crop_imm(&atlas,expression as u32*144,0,144,1440).to_image();
            assert!(sheet==column,"{expression:?} 与图集不一致，请重新拆分");
        }
    }
}
