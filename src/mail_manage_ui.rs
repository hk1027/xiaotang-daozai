//! 邮件列表整理状态与服务器操作反馈。
use crate::{app::{App,Cmd},mail::{Account,Item},mail_manage::{Action,Folder,Request,Snapshot,Completed}};
use std::collections::{HashMap,HashSet};
#[derive(Clone,Default,serde::Serialize,serde::Deserialize)]
struct LocalFlags{seen:Option<bool>,removed:bool}
fn identity(account:&Account)->String{
    let value=format!("{}:{}",account.provider.key(),account.credential_key());
    ring::digest::digest(&ring::digest::SHA256,value.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect()
}
#[derive(Default)]pub(crate) struct State{
    pub unread_only:bool,pub trash_open:bool,pub trash:Vec<Item>,
    pub data:HashMap<(String,Folder),Snapshot>,pub identities:HashMap<String,Account>,pub pending:HashSet<String>,
    pub next_refresh:f64,pub message:String,pub errors:Vec<String>,
    pub auto_read:Option<(String,u32)>,pub reader_trash:bool,
    reader_identity:Option<(Account,u32)>,
    local:HashMap<String,LocalFlags>,local_loaded:bool,
}
impl State{
    pub fn folder(&self)->Folder{if self.trash_open{Folder::Trash}else{Folder::Inbox}}
    pub fn seen(&self,item:&Item,folder:Folder)->Option<bool>{
        self.data.get(&(item.account.to_ascii_lowercase(),folder))?.entries.iter().find(|(uid,_)|*uid==item.uid).map(|(_,seen)|*seen)
    }
    pub fn ready(&self,account:&str,folder:Folder)->bool{self.pending.is_empty()&&self.data.contains_key(&(account.to_ascii_lowercase(),folder))}
}
impl App{
    fn mail_physical_folder(&self,folder:Folder)->Folder{if !self.settings.mail_sync_operations{Folder::Inbox}else{folder}}
    fn mail_local_key(&self,item:&Item)->Option<String>{
        let a=self.mail.accounts.iter().find(|a|a.email.eq_ignore_ascii_case(&item.account))?;
        if self.mail.manage.identities.get(&crate::mail::key(a))!=Some(a){return None;}
        let validity=self.mail.manage.data.get(&(crate::mail::key(a),Folder::Inbox))?.validity;
        Some(format!("{}:{validity}:{}",identity(a),item.uid))
    }
    pub(crate) fn mail_seen(&self,item:&Item,folder:Folder)->Option<bool>{
        if !self.settings.mail_sync_operations{
            if let Some(seen)=self.mail_local_key(item).and_then(|k|self.mail.manage.local.get(&k)).and_then(|f|f.seen){return Some(seen);}
        }
        self.mail.manage.seen(item,self.mail_physical_folder(folder))
    }
    pub(crate) fn mail_ready(&self,account:&str,folder:Folder)->bool{
        self.mail.manage.ready(account,self.mail_physical_folder(folder))&&self.mail.accounts.iter().any(|a|a.email.eq_ignore_ascii_case(account)&&self.mail.manage.identities.get(&crate::mail::key(a))==Some(a))
    }
    pub(crate) fn mail_open_reader(&mut self,item:&Item){
        let folder=self.mail.manage.folder();
        self.mail.manage.reader_trash=folder==Folder::Trash;
        let email=item.account.to_ascii_lowercase();
        self.mail.manage.reader_identity=self.mail.manage.identities.get(&email).cloned().zip(
            self.mail.manage.data.get(&(email,self.mail_physical_folder(folder)))
                .filter(|s|s.entries.iter().any(|(uid,_)|*uid==item.uid)).map(|s|s.validity));
        self.mail.reading=Some(item.clone());self.mail.reader_focus=true;
    }
    pub(crate) fn mail_reader_ready(&self,item:&Item,folder:Folder)->bool{
        let Some((account,validity))=&self.mail.manage.reader_identity else{return false;};
        let email=item.account.to_ascii_lowercase();
        self.mail_ready(&email,folder) && self.mail.manage.identities.get(&email)==Some(account)
            && self.mail.manage.data.get(&(email,self.mail_physical_folder(folder)))
                .is_some_and(|s|s.validity==*validity && s.entries.iter().any(|(uid,_)|*uid==item.uid))
    }
    pub(crate) fn mail_manage_mode_changed(&mut self){
        self.mail.manage.trash.clear();self.mail.manage.trash_open=false;self.mail.manage.auto_read=None;
        self.mail.open_uid=None;self.mail.reading=None;self.mail.manage.reader_identity=None;self.mail.manage.message.clear();self.mail.manage.next_refresh=0.0;
    }
    fn save_local_mail(&self)->Result<(),String>{
        let path=crate::model::config_dir().join("mail-local-state.json");
        let raw=serde_json::to_vec(&self.mail.manage.local).map_err(|_|"无法保存本机邮件状态")?;
        std::fs::create_dir_all(path.parent().unwrap()).and_then(|_|crate::model::atomic_write(&path,&raw)).map_err(|_|"本机邮件状态保存失败，请检查数据目录权限".into())
    }
    fn mail_local_action(&mut self,account:Option<&str>,folder:Folder,action:Action){
        let candidates:Vec<Item>=if action==Action::ReadAll{self.mail.recent.iter().filter(|m|account.is_none_or(|a|m.account.eq_ignore_ascii_case(a))).cloned().collect()}
            else{let uid=match action{Action::Read{uid,..}|Action::Move{uid}=>uid,_=>return};let rows=if folder==Folder::Inbox{&self.mail.recent}else{&self.mail.manage.trash};rows.iter().filter(|m|m.uid==uid&&account.is_some_and(|a|m.account.eq_ignore_ascii_case(a))).cloned().collect()};
        if candidates.is_empty(){self.mail.manage.message="没有可整理的邮件，请刷新列表".into();return;}
        let before=self.mail.manage.local.clone();
        for item in &candidates{
            let Some(k)=self.mail_local_key(item)else{self.mail.manage.local=before;self.mail.manage.message="请等待邮件状态同步完成".into();return;};
            let seen=self.mail_seen(item,folder);
            let flags=self.mail.manage.local.entry(k).or_default();
            match action{Action::Read{seen,..}=>flags.seen=Some(seen),Action::ReadAll=>flags.seen=Some(true),Action::Move{..}=>{flags.removed=folder==Folder::Inbox;if flags.seen.is_none(){flags.seen=seen;}},Action::Load=>{}}
        }
        if let Err(error)=self.save_local_mail(){self.mail.manage.local=before;self.mail.manage.message=error;return;}
        if matches!(action,Action::Move{..}){
            for item in candidates{
                let same=|m:&Item|m.uid==item.uid&&m.account.eq_ignore_ascii_case(&item.account);
                if folder==Folder::Inbox{self.mail.recent.retain(|m|!same(m));self.mail.manage.trash.retain(|m|!same(m));self.mail.manage.trash.insert(0,item);}
                else{self.mail.manage.trash.retain(|m|!same(m));self.mail.recent.retain(|m|!same(m));self.mail.recent.insert(0,item);}
            }
            self.mail.open_uid=None;self.mail.reading=None;
            self.mail.manage.message=if folder==Folder::Inbox{"已移到本机回收站"}else{"已恢复本机记录"}.into();
        }else{self.mail.manage.message="已更新本机状态".into();}
    }
    pub(crate) fn mail_rows(&self)->&[Item]{if self.mail.manage.trash_open{&self.mail.manage.trash}else{&self.mail.recent}}
    pub(crate) fn mail_manage_request(&mut self,account:Option<&str>,folder:Folder,action:Action){
        if !self.mail.manage.pending.is_empty(){return;}
        if action!=Action::Load&&!self.settings.mail_sync_operations{self.mail_local_action(account,folder,action);return;}
        let folder=self.mail_physical_folder(folder);
        let mut accounts:Vec<Account>=self.mail.accounts.iter().filter(|a|a.enabled&&account.is_none_or(|email|a.email.eq_ignore_ascii_case(email))).cloned().collect();
        if action==Action::ReadAll{accounts.retain(|a|self.mail.manage.data.contains_key(&(crate::mail::key(a),folder))&&self.mail.manage.identities.get(&crate::mail::key(a))==Some(a));}
        if accounts.is_empty(){return;}
        if action!=Action::Load && accounts.iter().any(|a|!self.mail.manage.data.contains_key(&(crate::mail::key(a),folder))||self.mail.manage.identities.get(&crate::mail::key(a))!=Some(a)){
            self.mail.manage.message="请等待邮件状态同步完成".into();return;
        }
        self.mail.manage.errors.clear();self.mail.manage.message=if action==Action::Load{"读取邮件状态…"}else{"同步中…"}.into();
        for account in accounts{
            let email=crate::mail::key(&account);
            let validity=self.mail.manage.data.get(&(email.clone(),folder)).filter(|_|self.mail.manage.identities.get(&email)==Some(&account)).map(|s|s.validity);
            let mut known:Vec<_>=if folder==Folder::Inbox{&self.mail.recent}else{&self.mail.manage.trash}.iter().filter(|m|m.account.eq_ignore_ascii_case(&email)).map(|m|m.uid).collect();
            if !self.settings.mail_sync_operations{known.extend(self.mail.manage.trash.iter().filter(|m|m.account.eq_ignore_ascii_case(&email)).map(|m|m.uid));}
            let mut extra:Vec<(u32,u32)>=Vec::new();
            if !self.settings.mail_sync_operations&&self.mail.manage.trash_open{
                let prefix=format!("{}:",identity(&account));
                extra=self.mail.manage.local.iter().filter(|(_,f)|f.removed).filter_map(|(k,_)|{let(v,uid)=k.strip_prefix(&prefix)?.split_once(':')?;Some((v.parse().ok()?,uid.parse().ok()?))}).collect();
                extra.sort_unstable_by(|a,b|b.1.cmp(&a.1));extra.truncate(60);
            }
            self.mail.manage.pending.insert(email);
            self.pending.push(Cmd::MailManage(Request{account,folder,validity,known,extra,action:action.clone()}));
        }
    }
    pub(crate) fn mail_reload_local(&mut self){
        self.mail.manage.local=std::fs::read(crate::model::config_dir().join("mail-local-state.json")).ok().and_then(|raw|serde_json::from_slice(&raw).ok()).unwrap_or_default();
        self.mail.manage.local_loaded=true;
    }
    pub(crate) fn mail_locally_removed(&self,item:&Item)->bool{
        !self.settings.mail_sync_operations&&self.mail_local_key(item).and_then(|k|self.mail.manage.local.get(&k)).is_some_and(|f|f.removed)
    }
    pub(crate) fn mail_manage_tick(&mut self){
        if !self.mail.manage.local_loaded{
            if !cfg!(test){self.mail_reload_local();}
            self.mail.manage.local_loaded=true;
        }
        if !self.mail.manage.pending.is_empty(){return;}
        if let Some((email,uid))=self.mail.manage.auto_read.take(){
            let folder=self.mail.manage.folder();
            let item=self.mail_rows().iter().find(|m|m.uid==uid&&m.account.eq_ignore_ascii_case(&email));
            if item.is_some_and(|m|self.mail_seen(m,folder)==Some(false)){
                self.mail_manage_request(Some(&email),folder,Action::Read{uid,seen:true});return;
            }
        }
        if !cfg!(test)&&self.t>=self.mail.manage.next_refresh{
            self.mail.manage.next_refresh=self.t+60.0;
            self.mail_manage_request(None,self.mail.manage.folder(),Action::Load);
        }
    }
    pub(crate) fn mail_managed(&mut self,done:Completed){
        let request=done.request;let email=crate::mail::key(&request.account);
        self.mail.manage.pending.remove(&email);
        if !self.mail.accounts.iter().any(|a|a==&request.account){return;}
        let snapshot=match done.result{Ok(s)=>s,Err(error)=>{
            self.mail.manage.errors.push(format!("{}：{error}",request.account.email));
            self.mail.manage.message=self.mail.manage.errors.join("；");return;
        }};
        let changed=self.mail.manage.data.get(&(email.clone(),request.folder)).is_some_and(|s|s.validity!=snapshot.validity);
        let mut snapshot=snapshot;
        let rows=if request.folder==Folder::Inbox{&mut self.mail.recent}else{&mut self.mail.manage.trash};
        rows.retain(|m|!m.account.eq_ignore_ascii_case(&email)||!changed&&snapshot.entries.iter().any(|(uid,_)|*uid==m.uid));
        for item in snapshot.items.drain(..){
            self.mail.privacy.observe(&item,false,self.t,chrono::Utc::now().timestamp());
            rows.retain(|m|m.uid!=item.uid||!m.account.eq_ignore_ascii_case(&email));rows.push(item);
        }
        rows.sort_by(|a,b|b.received_unix.cmp(&a.received_unix).then_with(||b.time.cmp(&a.time)).then_with(||b.uid.cmp(&a.uid)));
        if self.settings.mail_sync_operations||request.folder!=Folder::Inbox{rows.truncate(60);}
        if !self.settings.mail_sync_operations&&request.folder==Folder::Inbox{
            let prefix=format!("{}:{}:",identity(&request.account),snapshot.validity);
            if changed{self.mail.manage.trash.retain(|m|!m.account.eq_ignore_ascii_case(&email));}
            if !request.extra.is_empty(){self.mail.manage.trash.retain(|m|!m.account.eq_ignore_ascii_case(&email)||snapshot.entries.iter().any(|(uid,_)|*uid==m.uid));}
            let mut visible=Vec::new();
            for item in self.mail.recent.drain(..){
                if item.account.eq_ignore_ascii_case(&email)&&self.mail.manage.local.get(&format!("{prefix}{}",item.uid)).is_some_and(|f|f.removed){
                    self.mail.manage.trash.retain(|m|m.uid!=item.uid||!m.account.eq_ignore_ascii_case(&email));self.mail.manage.trash.push(item);
                }else{visible.push(item);}
            }
            visible.truncate(60);self.mail.recent=visible;
            self.mail.manage.trash.sort_by(|a,b|b.received_unix.cmp(&a.received_unix).then_with(||b.time.cmp(&a.time)).then_with(||b.uid.cmp(&a.uid)));self.mail.manage.trash.truncate(60);
        }
        let open_removed=self.mail.open_uid.as_ref().is_some_and(|(a,uid)|a.eq_ignore_ascii_case(&email)&&(changed||!snapshot.entries.iter().any(|(id,_)|id==uid)));
        if request.folder==self.mail_physical_folder(self.mail.manage.folder())&&open_removed{self.mail.open_uid=None;self.mail.manage.auto_read=None;}
        let reader_folder=if self.mail.manage.reader_trash{Folder::Trash}else{Folder::Inbox};
        if request.folder==self.mail_physical_folder(reader_folder) && self.mail.reading.as_ref().is_some_and(|m|
            m.account.eq_ignore_ascii_case(&email) && (self.mail.manage.reader_identity.as_ref().is_none_or(|(a,v)|a!=&request.account||*v!=snapshot.validity)
                || !snapshot.entries.iter().any(|(uid,_)|*uid==m.uid))) {self.mail.reading=None;self.mail.manage.reader_identity=None;}
        if matches!(request.action,Action::Move{..}){
            self.mail.manage.message=if request.folder==Folder::Inbox{"已移至回收站"}else{"已恢复到收件箱"}.into();
            if self.mail.manage.folder()!=request.folder{self.mail.manage.next_refresh=0.0;}
            if let Action::Move{uid}=request.action{
                if self.mail.reading.as_ref().is_some_and(|m|m.uid==uid&&m.account.eq_ignore_ascii_case(&email))&&self.mail.manage.reader_trash==(request.folder==Folder::Trash){self.mail.reading=None;}
            }
        }else if request.action==Action::ReadAll{self.mail.manage.message="已将邮箱未读邮件标为已读".into();}
        else{self.mail.manage.message=String::new();}
        self.mail.privacy.retain(self.mail.recent.iter().chain(&self.mail.manage.trash).chain(&self.mail.reading));
        self.mail.manage.identities.insert(email.clone(),request.account);
        self.mail.manage.data.insert((email,request.folder),snapshot);
        if !self.mail.manage.errors.is_empty(){self.mail.manage.message=self.mail.manage.errors.join("；");}
    }
    pub(crate) fn mail_open(&mut self,item:&Item){
        self.mail.open_uid=Some((item.account.clone(),item.uid));
        self.mail.manage.auto_read=Some((item.account.clone(),item.uid));
    }
    pub(crate) fn mail_toggle_trash(&mut self){
        self.mail.manage.trash_open=!self.mail.manage.trash_open;self.mail.open_uid=None;
        self.mail.manage.auto_read=None;self.mail.manage.next_refresh=0.0;self.mail.manage.message.clear();
    }
}
#[cfg(test)]mod tests{
    use super::*;
    fn account()->Account{Account{email:"test@example.com".into(),provider:crate::mail::Provider::Qq,custom:None,enabled:true}}
    #[test]fn failed_server_operation_keeps_the_message_and_reports_error(){
        let ctx=eframe::egui::Context::default();let mut app=App::new(&ctx,Default::default());let a=account();app.mail.accounts=vec![a.clone()];
        app.mail_manage_request(None,Folder::Inbox,Action::Load);
        assert!(app.mail.manage.pending.contains("test@example.com"));
        let request=match app.pending.pop().unwrap(){Cmd::MailManage(r)=>r,_=>panic!()};
        app.mail_managed(Completed{request,result:Err("服务器拒绝操作".into())});
        assert!(app.mail.manage.pending.is_empty());assert!(app.mail.manage.message.contains("服务器拒绝"));
    }
    #[test]fn local_trash_keeps_its_own_limit_when_inbox_also_has_recent_mail(){
        let ctx=eframe::egui::Context::default();let mut app=App::new(&ctx,Default::default());let a=account();app.mail.accounts=vec![a.clone()];app.settings.mail_sync_operations=false;app.mail.recent.clear();
        for uid in 1..=60{app.mail.manage.local.insert(format!("{}:7:{uid}",identity(&a)),LocalFlags{seen:Some(false),removed:true});}
        let items=(1..=80).map(|uid|Item{html:String::new().into(),account:a.email.clone(),from:String::new(),subject:String::new(),time:String::new(),code:None,received_unix:None,summary:"".into(),links:vec![],meeting:None,uid}).collect();
        app.mail_managed(Completed{request:Request{account:a,folder:Folder::Inbox,validity:None,known:vec![],extra:(1..=60).map(|uid|(7,uid)).collect(),action:Action::Load},result:Ok(Snapshot{validity:7,entries:(1..=80).map(|uid|(uid,false)).collect(),items})});
        assert_eq!(app.mail.recent.len(),20);assert_eq!(app.mail.manage.trash.len(),60,"本机回收站不能被收件箱的显示上限截断");
    }
    #[test]fn read_state_is_separate_for_each_folder_and_account(){
        let mut state=State::default();state.data.insert(("test@example.com".into(),Folder::Inbox),Snapshot{validity:1,entries:vec![(42,true)],items:vec![]});
        let item=Item{html:String::new().into(),account:"test@example.com".into(),from:String::new(),subject:String::new(),time:String::new(),code:None,received_unix:None,summary:"".into(),links:vec![],meeting:None,uid:42};
        assert_eq!(state.seen(&item,Folder::Inbox),Some(true));assert_eq!(state.seen(&item,Folder::Trash),None);
        let mut other=item;other.account="other@example.com".into();assert_eq!(state.seen(&other,Folder::Inbox),None);
    }
}

pub(crate) fn trash_icon(c:&crate::paint::Card,x:f32,y:f32,ink:eframe::egui::Color32){
    c.path(&[(x-4.4,y-2.8),(x-3.9,y+4.2),(x-3.5,y+5.2),(x-2.6,y+5.6),(x+2.6,y+5.6),(x+3.5,y+5.2),(x+3.9,y+4.2),(x+4.4,y-2.8)],1.25,ink);
    c.seg((x-5.8,y-4.8),(x+5.8,y-4.8),1.25,ink);
    c.path(&[(x-2.3,y-4.8),(x-2.3,y-6.1),(x-1.6,y-6.8),(x+1.6,y-6.8),(x+2.3,y-6.1),(x+2.3,y-4.8)],1.25,ink);
    c.seg((x-1.5,y-0.8),(x-1.5,y+2.7),1.05,ink);c.seg((x+1.5,y-0.8),(x+1.5,y+2.7),1.05,ink);
}
