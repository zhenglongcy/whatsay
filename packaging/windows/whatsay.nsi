; Whatsay Windows installer (NSIS 3).
; Mirrors packaging/windows/whatsay.iss: per-user install, same default
; directory (%LOCALAPPDATA%\Programs\Whatsay), same installed file set, so
; installing over an older Inno-based copy upgrades it in place.
;
; Build on Linux:  makensis packaging/windows/whatsay.nsi
;
; Besides the exe, the script expects these files next to it:
;   whatsay.ico        - app icon (in the repo)
;   libstdc++-6.dll, libgcc_s_seh-1.dll, libwinpthread-1.dll
;                        - MinGW C++ runtime closure the gnu-target exe needs
;                        (see File list below). From gcc-mingw-w64-x86-64-posix-runtime
;                        and mingw-w64-x86-64-dev .debs. Not committed (26 MB
;                        total). Windows finds them next to the exe.

!define APPNAME "Whatsay"
!define APPVERSION "0.20.2"
!define APPEXE "whatsay.exe"
!define PUBLISHER "Whatsay"
!define APPID "{F2512314-384A-4002-9933-AB840FD01639}"

!define SRCROOT "..\.."
!define BINARY "..\..\target\x86_64-pc-windows-gnu\release\whatsay.exe"

Name "${APPNAME}"
OutFile "whatsay-v${APPVERSION}-x86_64-pc-windows-gnu-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
RequestExecutionLevel user
Icon "${SRCROOT}\packaging\windows\whatsay.ico"
UninstallIcon "${SRCROOT}\packaging\windows\whatsay.ico"

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!define MUI_ABORTWARNING
!define MUI_ICON "${SRCROOT}\packaging\windows\whatsay.ico"
!define MUI_UNICON "${SRCROOT}\packaging\windows\whatsay.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APPEXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${APPNAME}"

; --- Data folder page -------------------------------------------------------
; Lets the user choose where chat history and downloaded files live.
; The choice is written to datadir.txt next to the exe; the app moves
; existing data there itself on next start (copying the archive key first).
; Default (empty choice) = the standard %LOCALAPPDATA%\paolino\zapfast.

Var DataDirDlg
Var DataDirText
Var DataDir

LangString DataDirTitle 1033 "Choose Data Location"
LangString DataDirTitle 2052 "选择数据保存位置"
LangString DataDirSub 1033 "Choose where Whatsay stores your chat history and downloaded files."
LangString DataDirSub 2052 "选择 Whatsay 保存聊天记录和文件的位置。"
LangString DataDirBrowse 1033 "Browse..."
LangString DataDirBrowse 2052 "浏览..."
LangString DataDirDefaultNote 1033 "Leave empty to use the default location."
LangString DataDirDefaultNote 2052 "留空则使用默认位置。"

Function DataDirCreate
  ; Previous choice, if any, becomes the starting value.
  StrCpy $DataDir ""
  ClearErrors
  FileOpen $0 "$INSTDIR\datadir.txt" r
  IfErrors no_prev_choice
    FileRead $0 $DataDir
    FileClose $0
  no_prev_choice:

  nsDialogs::Create 1018
  Pop $DataDirDlg
  ${If} $DataDirDlg == error
    Abort
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "$(DataDirTitle)" "$(DataDirSub)"
  ${NSD_CreateLabel} 0 0 100% 12u "$(DataDirSub)"
  Pop $0
  ${NSD_CreateLabel} 0 16u 100% 12u "$(DataDirDefaultNote)"
  Pop $0
  ${NSD_CreateText} 0 34u 75% 12u "$DataDir"
  Pop $DataDirText
  ${NSD_CreateButton} 77% 33u 23% 14u "$(DataDirBrowse)"
  Pop $0
  ${NSD_OnClick} $0 DataDirBrowse
  nsDialogs::Show
FunctionEnd

Function DataDirBrowse
  nsDialogs::SelectFolderDialog "$(DataDirTitle)" "$DataDir"
  Pop $0
  ${If} $0 != error
    ${NSD_SetText} $DataDirText $0
  ${EndIf}
FunctionEnd

Function DataDirLeave
  ${NSD_GetText} $DataDirText $DataDir
FunctionEnd

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
Page custom DataDirCreate DataDirLeave
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "${APPNAME}" SecMain
  SetOutPath "$INSTDIR"

  ; If a copy is running, ask the user to close it before replacing files.
  ; (Inno's CloseApplications=yes equivalent, polite version.)
  retry:
    ClearErrors
    FileOpen $0 "$INSTDIR\${APPEXE}" a
    IfErrors 0 done
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION \
        "${APPNAME} seems to be running. Please close it, then click Retry." \
        IDRETRY retry
      Quit
  done:
    FileClose $0

  File "${BINARY}"
  ; MinGW C++ runtime: the full DLL closure the gnu-target exe needs.
  ; whatsay.exe -> libstdc++-6.dll -> { libgcc_s_seh-1.dll, libwinpthread-1.dll }
  ; (verified with objdump; both levels bottom out at system DLLs only)
  File "libstdc++-6.dll"
  File "libgcc_s_seh-1.dll"
  File "libwinpthread-1.dll"
  File "${SRCROOT}\packaging\windows\whatsay.ico"
  ; Updaters up to 0.16.5 relaunch the executable they were started from, so an
  ; update begun as fastsapp.exe needs that file to come back. The app deletes
  ; the copy once it starts as whatsay.exe with no update running.
  CopyFiles /SILENT "$INSTDIR\${APPEXE}" "$INSTDIR\fastsapp.exe"
  File "${SRCROOT}\README.md"
  File "${SRCROOT}\LICENSE"
  File "${SRCROOT}\THIRD-PARTY-NOTICES.md"
  File "${SRCROOT}\packaging\windows\whatsay-installer.txt"
  ; The updater detects an installed copy by the slug's installer
  ; marker, so ship that name too (content: zapfast-installer-v1).
  File "${SRCROOT}\packaging\windows\zapfast-installer.txt"

  ; Record the data-location choice for the app (datadir.txt next to the
  ; exe). Line 1 = desired root ("default"/empty = platform directories),
  ; line 2 = previously chosen root for migrating existing data.
  ; $DataDir comes from the data page; empty means default.
  StrCpy $1 "" ; previous line 1
  ClearErrors
  FileOpen $0 "$INSTDIR\datadir.txt" r
  IfErrors no_prev_datadir
    FileRead $0 $1
    FileClose $0
  no_prev_datadir:
  ; Trim trailing CR/LF from the previous value for comparison.
  trim_prev:
    StrCpy $2 $1 1 -1
    StrCmp $2 "$\r" trim_prev_cut
    StrCmp $2 "$\n" trim_prev_cut trim_prev_done
  trim_prev_cut:
    StrCpy $1 $1 -1
    Goto trim_prev
  trim_prev_done:
  ${If} $DataDir == ""
    ${If} $1 == ""
      Delete "$INSTDIR\datadir.txt" ; already default, keep it clean
    ${ElseIf} $1 != "default"
      ; Reverting to default: the app migrates the old root back.
      FileOpen $0 "$INSTDIR\datadir.txt" w
      FileWrite $0 "default$\r$\n$1"
      FileClose $0
    ${EndIf}
  ${ElseIf} $DataDir != $1
    FileOpen $0 "$INSTDIR\datadir.txt" w
    FileWrite $0 "$DataDir$\r$\n$1"
    FileClose $0
  ${EndIf}

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Start menu shortcut (always) + desktop shortcut (optional).
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" \
    "$INSTDIR\${APPEXE}" "" "$INSTDIR\whatsay.ico" 0
  ; Tell Explorer the icon changed so shortcuts do not keep showing a
  ; stale cached icon.
  System::Call 'shell32.dll::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'

  ; Add/Remove Programs entry (per-user).
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
    "DisplayName" "${APPNAME} ${APPVERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
    "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
    "DisplayIcon" "$INSTDIR\whatsay.ico"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
    "DisplayVersion" "${APPVERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
    "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
    "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
    "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
    "NoRepair" 1
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" \
    "$INSTDIR\${APPEXE}" "" "$INSTDIR\whatsay.ico" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "${APPNAME} ${APPVERSION}"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Create a desktop shortcut"
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Delete "$INSTDIR\${APPEXE}"
  Delete "$INSTDIR\libstdc++-6.dll"
  Delete "$INSTDIR\libgcc_s_seh-1.dll"
  Delete "$INSTDIR\libwinpthread-1.dll"
  Delete "$INSTDIR\fastsapp.exe"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.md"
  Delete "$INSTDIR\whatsay-installer.txt"
  Delete "$INSTDIR\whatsay.ico"
  Delete "$INSTDIR\datadir.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}"
SectionEnd
