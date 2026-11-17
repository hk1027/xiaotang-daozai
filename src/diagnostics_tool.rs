//! 收集诊断：诊断脚本内嵌在程序里，`daozai.exe --collect-diagnostics` 直接运行，不依赖安装目录里的附带文件。
//!
//! 两个入口共用这里：
//! - 「设置 → 关于 → 收集诊断」：岛仔能打开时用；
//! - 开始菜单「收集岛仔诊断」（免安装包里是同名的 cmd）：岛仔启动即崩溃、卡死、找不到窗口时用。
//!   这条路在单实例检查、创建窗口和初始化显卡之前就处理，显卡驱动出问题也能收集。
#[cfg(windows)]
use std::path::{Path,PathBuf};

pub const ARG:&str="--collect-diagnostics";
const SCRIPT:&[u8]=include_bytes!("../scripts/collect-diagnostics.ps1");

/// 命令行里是否要求收集诊断。
pub fn requested(mut args:impl Iterator<Item=String>)->bool {args.any(|arg|arg==ARG)}

/// 命令行入口：释放脚本、在新控制台里收集、结束后在资源管理器里选中诊断包。
#[cfg(windows)]
pub fn run(data_dir:&Path)->Result<(),String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE:u32=0x0000_0010;
    let work=std::env::temp_dir().join(format!("daozai-diagnostics-tool-{}",std::process::id()));
    std::fs::create_dir_all(&work).map_err(|e|format!("无法创建临时目录：{e}"))?;
    let script=work.join("collect-diagnostics.ps1");
    let result=(||{
        // 脚本自带 UTF-8 BOM，Windows PowerShell 5.1 才能正确读中文。
        std::fs::write(&script,SCRIPT).map_err(|e|format!("无法写入诊断脚本：{e}"))?;
        let output=dirs_desktop().unwrap_or_else(||data_dir.to_path_buf());
        let status=std::process::Command::new("powershell.exe")
            .args(["-NoProfile","-ExecutionPolicy","Bypass","-File"]).arg(&script)
            .arg("-DataDirectory").arg(data_dir).arg("-OutputDirectory").arg(output).arg("-RevealArchive")
            .creation_flags(CREATE_NEW_CONSOLE).status().map_err(|e|format!("无法启动 PowerShell：{e}"))?;
        if !status.success(){return Err("诊断收集未完成".into());}
        Ok(())
    })();
    let _=std::fs::remove_file(&script);let _=std::fs::remove_dir(&work);
    result
}

/// 当前用户的桌面（诊断包默认放这里，方便用户找到并发送）。
#[cfg(windows)]
fn dirs_desktop()->Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Desktop,SHGetKnownFolderPath,KF_FLAG_DEFAULT};
    unsafe {
        let path=SHGetKnownFolderPath(&FOLDERID_Desktop,KF_FLAG_DEFAULT,None).ok()?;
        let text=path.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const _));
        text.map(PathBuf::from)
    }
}

/// 应用内按钮：另起一个 `daozai.exe --collect-diagnostics`，界面不等待，也不受当前窗口状态影响。
pub fn launch()->std::io::Result<()> {
    let exe=std::env::current_exe()?;
    std::process::Command::new(exe).arg(ARG).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map(|_|())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_explicit_switch_starts_collection(){
        assert!(requested(["daozai.exe".to_string(),ARG.to_string()].into_iter()));
        assert!(!requested(["daozai.exe".to_string(),"--settings".to_string()].into_iter()));
    }
    #[test]
    fn embedded_script_keeps_bom_and_collects_music_gaps(){
        assert!(SCRIPT.starts_with(&[0xEF,0xBB,0xBF]),"Windows PowerShell 5.1 需要 BOM 才能读中文");
        let text=String::from_utf8_lossy(SCRIPT);
        assert!(text.contains("music-gaps.log") && text.contains("param("));
    }
}
