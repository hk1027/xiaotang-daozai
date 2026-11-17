; -*- coding: utf-8 -*-
Unicode true
!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "FileFunc.nsh"

!ifndef APP_ID
  !define APP_ID "Daozai"
!endif
!ifndef APP_NAME
  !define APP_NAME "岛仔"
!endif
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
Name "${APP_NAME}"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_ID}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
ManifestDPIAware true
SetCompressor /SOLID lzma
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=2052 "ProductName" "${APP_NAME}"
VIAddVersionKey /LANG=2052 "FileDescription" "${APP_NAME}安装程序"
VIAddVersionKey /LANG=2052 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "LegalCopyright" "岛仔贡献者"
!define MUI_ICON "${PROJECT_DIR}\assets\daozai.ico"
!define MUI_UNICON "${PROJECT_DIR}\assets\daozai.ico"
!define MUI_ABORTWARNING
Var BrandIcon
Var PathInput
Var DesktopCheckbox
Var AutoStartCheckbox
Var DesktopChoice
Var AutoStartChoice
Page custom InstallPage InstallPageLeave
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TITLE "岛仔已安装"
!define MUI_FINISHPAGE_TEXT "音乐、提醒和日常小事，现在都在桌面上的小岛里。"
!define MUI_FINISHPAGE_RUN "$INSTDIR\daozai.exe"
!define MUI_FINISHPAGE_RUN_TEXT "打开岛仔"
!insertmacro MUI_PAGE_FINISH
!define MUI_UNCONFIRMPAGE_TEXT_TOP "将卸载岛仔。设置、待办和连接记录会保留，重新安装后仍可使用。"
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
BrandingText "岛仔 · Daozai"
SetFont "Microsoft YaHei UI" 9

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "岛仔需要 Windows 10 或 Windows 11 的 64 位系统。" /SD IDOK
    SetErrorLevel 1
    Quit
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "岛仔需要 Windows 10 或 Windows 11。" /SD IDOK
    SetErrorLevel 1
    Quit
  ${EndIf}
  SetShellVarContext current
  SetRegView 64
  StrCpy $DesktopChoice 1
  StrCpy $AutoStartChoice 0
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 != ""
    IfFileExists "$DESKTOP\${APP_NAME}.lnk" +2 0
      StrCpy $DesktopChoice 0
  ${EndIf}
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${If} $0 != ""
    StrCpy $AutoStartChoice 1
  ${EndIf}
  ; 静默安装使用同一组选项；/D 路径由 NSIS 自身处理。
  ${GetParameters} $0
  ${GetOptions} $0 "/DESKTOP=" $1
  ${If} $1 == "0"
  ${OrIf} $1 == "1"
    StrCpy $DesktopChoice $1
  ${EndIf}
  ${GetOptions} $0 "/AUTOSTART=" $1
  ${If} $1 == "0"
  ${OrIf} $1 == "1"
    StrCpy $AutoStartChoice $1
  ${EndIf}
FunctionEnd

Function InstallPage
  !insertmacro MUI_HEADER_TEXT "欢迎使用岛仔" "Windows 顶部的桌面伙伴"
  nsDialogs::Create 1018
  Pop $0
  InitPluginsDir
  File /oname=$PLUGINSDIR\brand.ico "${PROJECT_DIR}\assets\daozai.ico"
  ${NSD_CreateIcon} 0 6u 44u 44u ""
  Pop $0
  System::Call 'user32::LoadImageW(p 0, w "$PLUGINSDIR\brand.ico", i 1, i 64, i 64, i 0x10) p .r1'
  StrCpy $BrandIcon $1
  SendMessage $0 0x170 $1 0
  ${NSD_CreateLabel} 58u 8u 240u 22u "${APP_NAME}"
  Pop $0
  CreateFont $1 "Microsoft YaHei UI" 18 700
  SendMessage $0 ${WM_SETFONT} $1 0
  ${NSD_CreateLabel} 112u 14u 188u 14u "v${APP_VERSION} · 升级保留设置和记录"
  Pop $0
  ${NSD_CreateLabel} 58u 34u 240u 20u "让重要的信息，轻轻出现在屏幕顶部。"
  Pop $0
  ${NSD_CreateLabel} 0 58u 300u 16u "音乐歌词 · 邮件验证码 · 日程提醒 · AI 额度"
  Pop $0
  ${NSD_CreateLabel} 0 80u 300u 12u "安装位置"
  Pop $0
  ${NSD_CreateDirRequest} 0 96u 245u 16u "$INSTDIR"
  Pop $PathInput
  ${NSD_CreateBrowseButton} 250u 96u 50u 16u "浏览…"
  Pop $0
  ${NSD_OnClick} $0 BrowseDirectory
  ${NSD_CreateCheckbox} 0 122u 150u 14u "创建桌面快捷方式"
  Pop $DesktopCheckbox
  ${NSD_SetState} $DesktopCheckbox $DesktopChoice
  ${NSD_CreateCheckbox} 165u 122u 135u 14u "开机自启"
  Pop $AutoStartCheckbox
  ${NSD_SetState} $AutoStartCheckbox $AutoStartChoice
  GetDlgItem $0 $HWNDPARENT 1
  SendMessage $0 ${WM_SETTEXT} 0 "STR:一键安装"
  nsDialogs::Show
FunctionEnd

; 只接受盘符或 UNC 共享下的完整子目录。先规范化，避免把空输入或盘符补成安装路径。
Function CheckFolderPath
  ${GetRoot} "$INSTDIR" $0
  ${If} $0 != ""
    StrLen $1 $0
    StrCpy $2 "$INSTDIR" "" $1
    StrCpy $1 $2 1
    ${If} $1 == "\"
    ${AndIf} $2 != "\"
      System::Call 'kernel32::GetFullPathNameW(w "$INSTDIR", i ${NSIS_MAX_STRLEN}, w .r2, p 0) i .r0'
      ${If} $0 == 0
      ${OrIf} $0 >= ${NSIS_MAX_STRLEN}
        Goto invalid_path
      ${EndIf}
      System::Call 'shlwapi::PathIsRootW(w "$2") i .r0'
      ${If} $0 == 0
        StrCpy $INSTDIR $2
        StrCpy $0 1
        Return
      ${EndIf}
    ${EndIf}
  ${EndIf}
  invalid_path:
  StrCpy $0 0
FunctionEnd

; 选中的文件夹里已有别的东西（又不是岛仔自己的）时，在其中新建 Daozai 子文件夹，
; 避免把整个共享文件夹登记成岛仔的安装位置（软件管家会把整个文件夹的大小算到岛仔头上）。
; 文件夹本身叫 Daozai、里面已有岛仔（原地升级）、不存在或为空（用户特意准备的）时原样使用。
Function EnsureOwnFolder
  Call CheckFolderPath
  ${If} $0 == 0
    Return
  ${EndIf}
  StrCpy $0 $INSTDIR 1 -1
  ${If} $0 == "\"
    StrCpy $INSTDIR $INSTDIR -1
  ${EndIf}
  ${GetFileName} "$INSTDIR" $0
  ${If} $0 == "${APP_ID}"
    Return
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\daozai.exe"
    Return
  ${EndIf}
  ClearErrors
  FindFirst $1 $2 "$INSTDIR\*.*"
  ${DoWhile} $2 != ""
    ${If} $2 != "."
    ${AndIf} $2 != ".."
      FindClose $1
      StrCpy $INSTDIR "$INSTDIR\${APP_ID}"
      Return
    ${EndIf}
    FindNext $1 $2
  ${Loop}
  FindClose $1
  ClearErrors
FunctionEnd

Function BrowseDirectory
  Pop $0
  ${NSD_GetText} $PathInput $0
  nsDialogs::SelectFolderDialog "选择岛仔安装文件夹" "$0"
  Pop $0
  ${If} $0 != "error"
    StrCpy $INSTDIR $0
    Call EnsureOwnFolder
    ${NSD_SetText} $PathInput "$INSTDIR"
  ${EndIf}
FunctionEnd

Function ValidateDirectory
  Call CheckFolderPath
  ${If} $0 == 0
    Goto invalid
  ${EndIf}
  ClearErrors
  CreateDirectory "$INSTDIR"
  IfErrors invalid
  GetFullPathName $INSTDIR "$INSTDIR"
  IfErrors invalid
  GetTempFileName $0 "$INSTDIR"
  IfErrors invalid
  Delete "$0"
  SetErrorLevel 0
  Return
  invalid:
    MessageBox MB_OK|MB_ICONEXCLAMATION "请选择可写入的完整安装文件夹，例如 D:\Apps\Daozai。不要选择磁盘根目录或需要管理员权限的文件夹。" /SD IDOK
    SetErrorLevel 1
    Abort
FunctionEnd

Function InstallPageLeave
  ${NSD_GetText} $PathInput $INSTDIR
  ${NSD_GetState} $DesktopCheckbox $DesktopChoice
  ${NSD_GetState} $AutoStartCheckbox $AutoStartChoice
  Call EnsureOwnFolder
  ${NSD_SetText} $PathInput "$INSTDIR"
  Call ValidateDirectory
FunctionEnd

Function .onGUIEnd
  System::Call 'user32::DestroyIcon(p $BrandIcon)'
FunctionEnd

; 程序运行时请用户正常退出，不强制结束进程或写入正在使用的程序。
!macro ClosedCheck Prefix
Function ${Prefix}CheckClosed
  retry:
  System::Call 'kernel32::OpenMutexW(i 0x100000, i 0, w "Local\${APP_ID}.SingleInstance") p .r0'
  ${If} $0 != 0
    System::Call 'kernel32::CloseHandle(p r0)'
    MessageBox MB_RETRYCANCEL|MB_ICONINFORMATION "请先从系统托盘退出${APP_NAME}，然后点击重试。" /SD IDCANCEL IDRETRY retry
    SetErrorLevel 2
    Abort
  ${EndIf}
FunctionEnd
!macroend
!insertmacro ClosedCheck ""
!insertmacro ClosedCheck "un."

Section "安装"
  Call CheckClosed
  ; 静默安装的 /D 路径同样遵循专用文件夹规则。
  Call EnsureOwnFolder
  Call ValidateDirectory
  SetOutPath "$INSTDIR"
  SetOverwrite on
  File /oname=daozai.exe "${SOURCE_EXE}"
  File "${PROJECT_DIR}\LICENSE"
  File "${PROJECT_DIR}\THIRD_PARTY_NOTICES.txt"
  ; 诊断脚本已内嵌在程序里（daozai.exe --collect-diagnostics），清掉测试版留下的单独诊断文件。
  Delete "$INSTDIR\收集岛仔诊断.cmd"
  Delete "$INSTDIR\scripts\collect-diagnostics.ps1"
  RMDir "$INSTDIR\scripts"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  ${If} $DesktopChoice == 1
    CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\daozai.exe"
  ${Else}
    Delete "$DESKTOP\${APP_NAME}.lnk"
  ${EndIf}
  ${If} $AutoStartChoice == 1
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}" '"$INSTDIR\daozai.exe"'
  ${Else}
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${EndIf}
  CreateShortcut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\daozai.exe"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}设置.lnk" "$INSTDIR\daozai.exe" "--settings"
  ; 岛仔打不开、卡死时的备用入口；能打开时用「设置 → 关于 → 收集诊断」。
  CreateShortcut "$SMPROGRAMS\收集${APP_NAME}诊断.lnk" "$INSTDIR\daozai.exe" "--collect-diagnostics" "$INSTDIR\daozai.exe" 0
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME} Daozai"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "岛仔"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\daozai.exe,0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ; 登记岛仔自己文件的大小（KB），软件管家就不会去统计整个安装文件夹。
  ${GetSize} "$INSTDIR" "/M=daozai.exe /S=0K /G=0" $0 $1 $2
  ${GetSize} "$INSTDIR" "/M=Uninstall.exe /S=0K /G=0" $3 $1 $2
  IntOp $0 $0 + $3
  ${GetSize} "$INSTDIR" "/M=THIRD_PARTY_NOTICES.txt /S=0K /G=0" $3 $1 $2
  IntOp $0 $0 + $3
  IntOp $0 $0 + 8
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0
  Delete "$INSTDIR\uninstall.ps1"
SectionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
FunctionEnd

Section "Uninstall"
  Call un.CheckClosed
  Delete "$INSTDIR\daozai.exe"
  Delete "$INSTDIR\daozai-previous.exe"
  Delete "$INSTDIR\daozai-v0.2-backup.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\uninstall.ps1"
  Delete "$INSTDIR\收集岛仔诊断.cmd"
  Delete "$INSTDIR\scripts\collect-diagnostics.ps1"
  RMDir "$INSTDIR\scripts"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}设置.lnk"
  Delete "$SMPROGRAMS\收集${APP_NAME}诊断.lnk"
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${If} $0 == '"$INSTDIR\daozai.exe"'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${EndIf}
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ; 只移除空的安装目录，用户数据和用户自行放入的文件都保留。
  RMDir "$INSTDIR"
SectionEnd
