; Whatsay Windows installer (NSIS 3).
; Mirrors packaging/windows/whatsay.iss: per-user install, same default
; directory (%LOCALAPPDATA%\Programs\Whatsay), same installed file set, so
; installing over an older Inno-based copy upgrades it in place.
;
; Build on Linux:  makensis packaging/windows/whatsay.nsi

!define APPNAME "Whatsay"
!define APPVERSION "0.19.0"
!define APPEXE "whatsay.exe"
!define PUBLISHER "Carmine Paolino"
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
!define MUI_ABORTWARNING
!define MUI_ICON "${SRCROOT}\packaging\windows\whatsay.ico"
!define MUI_UNICON "${SRCROOT}\packaging\windows\whatsay.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APPEXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${APPNAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
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
  File "${SRCROOT}\packaging\windows\whatsay.ico"
  ; Updaters up to 0.16.5 relaunch the executable they were started from, so an
  ; update begun as fastsapp.exe needs that file to come back. The app deletes
  ; the copy once it starts as whatsay.exe with no update running.
  CopyFiles /SILENT "$INSTDIR\${APPEXE}" "$INSTDIR\fastsapp.exe"
  File "${SRCROOT}\README.md"
  File "${SRCROOT}\LICENSE"
  File "${SRCROOT}\THIRD-PARTY-NOTICES.md"
  File "${SRCROOT}\packaging\windows\whatsay-installer.txt"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Start menu shortcut (always) + desktop shortcut (optional).
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" \
    "$INSTDIR\${APPEXE}" "" "$INSTDIR\whatsay.ico" 0

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
  Delete "$INSTDIR\fastsapp.exe"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.md"
  Delete "$INSTDIR\whatsay-installer.txt"
  Delete "$INSTDIR\whatsay.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}"
SectionEnd
