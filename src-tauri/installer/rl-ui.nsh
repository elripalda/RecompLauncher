; RecompLauncher's own installer screens, on top of Tauri's installer script (tools/brand-installer.py
; puts them in). One small window with no frame: Install (or Update, or Repair when it's the same
; version, with Uninstall under it) with where it goes, a progress bar while it installs, then Open
; RecompLauncher. The pictures (bmp/*.bmp) carry the words and buttons; clear areas over the buttons
; take the clicks. Five picture sets, one per Windows scale. The uninstaller's screens are in
; rl-un.nsh and share what's here.

!include nsDialogs.nsh
!include LogicLib.nsh
!include WinMessages.nsh
!include WordFunc.nsh

!define RL_W 720
!define RL_H 460
!define RL_BG 0B1220

Var RLDpi
Var RLSet
Var RLDlg
Var RLPathLbl
Var RLFont
Var RLFontB
Var RLRunIt
Var RLHad

; Logical pixels to this screen's pixels.
!macro RLPX OUT N
  IntOp ${OUT} ${N} * $RLDpi
  IntOp ${OUT} ${OUT} / 96
!macroend

!macro RLPICS SET
  File "/oname=$PLUGINSDIR\rl-welcome.bmp" "${RLBMP}\welcome@${SET}.bmp"
  File "/oname=$PLUGINSDIR\rl-update.bmp" "${RLBMP}\update@${SET}.bmp"
  File "/oname=$PLUGINSDIR\rl-progress.bmp" "${RLBMP}\progress@${SET}.bmp"
  File "/oname=$PLUGINSDIR\rl-done.bmp" "${RLBMP}\done@${SET}.bmp"
  File "/oname=$PLUGINSDIR\rl-repair.bmp" "${RLBMP}\repair@${SET}.bmp"
!macroend

!macro RLUNPICS SET
  File "/oname=$PLUGINSDIR\rl-unconfirm.bmp" "${RLBMP}\unconfirm@${SET}.bmp"
  File "/oname=$PLUGINSDIR\rl-unprogress.bmp" "${RLBMP}\unprogress@${SET}.bmp"
  File "/oname=$PLUGINSDIR\rl-unbye.bmp" "${RLBMP}\unbye@${SET}.bmp"
  File "/oname=$PLUGINSDIR\rl-chk-off.bmp" "${RLBMP}\chk-off@${SET}.bmp"
  File "/oname=$PLUGINSDIR\rl-chk-on.bmp" "${RLBMP}\chk-on@${SET}.bmp"
!macroend

; A picture filling a dialog, behind everything else on it.
!macro RLBACK PARENT FILE STYLE
  !insertmacro RLPX $1 ${RL_W}
  !insertmacro RLPX $2 ${RL_H}
  System::Call 'user32::CreateWindowEx(i 0, t "STATIC", t "", i ${STYLE}, i 0, i 0, i r1, i r2, p ${PARENT}, p 0, p 0, p 0) p.r3'
  System::Call 'user32::LoadImage(p 0, t "$PLUGINSDIR\${FILE}", i 0, i 0, i 0, i 0x10) p.r4'
  SendMessage $3 0x172 0 $4
  System::Call 'user32::SetWindowPos(p r3, p 1, i 0, i 0, i 0, i 0, i 0x13)'
!macroend

; A clear area that takes clicks, over a button drawn in the picture.
!macro RLHOT X Y W H FUNC
  !insertmacro RLPX $1 ${X}
  !insertmacro RLPX $2 ${Y}
  !insertmacro RLPX $3 ${W}
  !insertmacro RLPX $4 ${H}
  nsDialogs::CreateControl STATIC 0x50000100 0 $1 $2 $3 $4 ""
  Pop $0
  SetCtlColors $0 "" transparent
  ${NSD_OnClick} $0 ${FUNC}
!macroend

; Words on the dark background, centred.
!macro RLTEXT OUTVAR X Y W H TEXT FG FONT NOTIFY
  !insertmacro RLPX $1 ${X}
  !insertmacro RLPX $2 ${Y}
  !insertmacro RLPX $3 ${W}
  !insertmacro RLPX $4 ${H}
  !if "${NOTIFY}" == "1"
    nsDialogs::CreateControl STATIC 0x50008101 0 $1 $2 $3 $4 "${TEXT}"
  !else
    nsDialogs::CreateControl STATIC 0x50008001 0 $1 $2 $3 $4 "${TEXT}"
  !endif
  Pop ${OUTVAR}
  SetCtlColors ${OUTVAR} ${FG} ${RL_BG}
  SendMessage ${OUTVAR} ${WM_SETFONT} ${FONT} 1
!macroend

!macro RLGUIINIT UN PICS
Function ${UN}RLGuiInit
  InitPluginsDir
  ; This screen's scale.
  StrCpy $0 0
  System::Call 'user32::GetDpiForWindow(p $HWNDPARENT) i.r0 ?e'
  Pop $1
  ${If} $0 < 96
    System::Call 'user32::GetDC(p 0) p.r1'
    System::Call 'gdi32::GetDeviceCaps(p r1, i 88) i.r0'
    System::Call 'user32::ReleaseDC(p 0, p r1)'
  ${EndIf}
  ${If} $0 < 96
    StrCpy $0 96
  ${EndIf}
  StrCpy $RLDpi $0
  IntOp $1 $0 * 100
  IntOp $1 $1 / 96
  ${If} $1 <= 100
    StrCpy $RLSet 100
    !insertmacro ${PICS} 100
  ${ElseIf} $1 <= 125
    StrCpy $RLSet 125
    !insertmacro ${PICS} 125
  ${ElseIf} $1 <= 150
    StrCpy $RLSet 150
    !insertmacro ${PICS} 150
  ${ElseIf} $1 <= 175
    StrCpy $RLSet 175
    !insertmacro ${PICS} 175
  ${Else}
    StrCpy $RLSet 200
    !insertmacro ${PICS} 200
  ${EndIf}
  ; Pictures are drawn for these exact sizes, so the window is that size at this scale.
  StrCpy $RLDpi $RLSet
  IntOp $RLDpi $RLDpi * 96
  IntOp $RLDpi $RLDpi / 100
  CreateFont $RLFont "Segoe UI" 9 400
  CreateFont $RLFontB "Segoe UI Semibold" 9 600
  ; No frame or title bar: a popup that keeps its taskbar button and Alt+F4.
  System::Call 'user32::SetWindowLong(p $HWNDPARENT, i -16, i 0x820A0000)'
  System::Call 'user32::GetWindowLong(p $HWNDPARENT, i -20) i.r0'
  IntOp $0 $0 & 0xFFFFFCFE
  IntOp $0 $0 | 0x40000
  System::Call 'user32::SetWindowLong(p $HWNDPARENT, i -20, i r0)'
  ; The installer's own buttons, header and lines stay out of sight; the pages take the whole window.
  !insertmacro RLPX $5 ${RL_W}
  !insertmacro RLPX $6 ${RL_H}
  ${ForEach} $7 1 3 + 1
    GetDlgItem $0 $HWNDPARENT $7
    ShowWindow $0 ${SW_HIDE}
  ${Next}
  ${ForEach} $7 1028 1050 + 1
    GetDlgItem $0 $HWNDPARENT $7
    ShowWindow $0 ${SW_HIDE}
  ${Next}
  GetDlgItem $0 $HWNDPARENT 1256
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $HWNDPARENT 1018
  System::Call 'user32::SetWindowPos(p r0, p 0, i 0, i 0, i r5, i r6, i 0x14)'
  ; Centred on the screen's work area.
  System::Call '*(i,i,i,i) p.r1'
  System::Call 'user32::SystemParametersInfo(i 48, i 0, p r1, i 0)'
  System::Call '*$1(i.r2, i.r3, i.r4, i.r8)'
  System::Free $1
  IntOp $4 $4 - $2
  IntOp $4 $4 - $5
  IntOp $4 $4 / 2
  IntOp $4 $4 + $2
  IntOp $8 $8 - $3
  IntOp $8 $8 - $6
  IntOp $8 $8 / 2
  IntOp $8 $8 + $3
  System::Call 'user32::SetWindowPos(p $HWNDPARENT, p 0, i r4, i r8, i r5, i r6, i 0x24)'
  SetCtlColors $HWNDPARENT "" ${RL_BG}
  ; Rounded corners on Windows 11 (ignored before).
  System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 33, *i 2, i 4)'
  ; A dark frame colour on Windows 11.
  System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 34, *i 0x00332214, i 4)'
FunctionEnd
!macroend
!insertmacro RLGUIINIT "" RLPICS
!insertmacro RLGUIINIT "un." RLUNPICS

!macro RLSHARED UN
; The installer's own Back, Next and Cancel: every page shows them again, so every page hides them.
Function ${UN}RLHideButtons
  GetDlgItem $0 $HWNDPARENT 1046
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $HWNDPARENT 1
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $HWNDPARENT 2
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $HWNDPARENT 3
  ShowWindow $0 ${SW_HIDE}
FunctionEnd

Function ${UN}RLNext
  SendMessage $HWNDPARENT 0x408 1 0
FunctionEnd

Function ${UN}RLQuit
  SendMessage $HWNDPARENT ${WM_COMMAND} 2 0
FunctionEnd

!macroend
!insertmacro RLSHARED ""
!insertmacro RLSHARED "un."

Function RLShowPath
  ${NSD_SetText} $RLPathLbl "Installs to $INSTDIR"
FunctionEnd

Function RLPick
  nsDialogs::SelectFolderDialog "Where should RecompLauncher go?" "$INSTDIR"
  Pop $0
  ${If} $0 == "error"
  ${OrIf} $0 == ""
    Return
  ${EndIf}
  ; The folder they picked, with RecompLauncher's own folder in it.
  ${GetFileName} "$0" $1
  ${If} $1 == "${PRODUCTNAME}"
    StrCpy $INSTDIR "$0"
  ${Else}
    StrCpy $INSTDIR "$0\${PRODUCTNAME}"
  ${EndIf}
  Call RLShowPath
FunctionEnd

; 1. Install, or Update when it's already there.
Function RLWelcome
  ${If} $PassiveMode = 1
    Abort
  ${EndIf}
  Call RLHideButtons
  nsDialogs::Create 1018
  Pop $RLDlg
  SetCtlColors $RLDlg "" ${RL_BG}
  ReadRegStr $RLHad SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${If} $RLHad == ""
    !insertmacro RLBACK $RLDlg rl-welcome.bmp 0x5000000E
  ${ElseIf} $RLHad == "${VERSION}"
    !insertmacro RLBACK $RLDlg rl-repair.bmp 0x5000000E
  ${Else}
    !insertmacro RLBACK $RLDlg rl-update.bmp 0x5000000E
  ${EndIf}
  !insertmacro RLHOT 270 252 180 64 RLNext
  !insertmacro RLHOT 668 6 44 44 RLQuit
  ${If} $RLHad == ""
    !insertmacro RLTEXT $RLPathLbl 60 336 600 20 "" A9B4C4 $RLFont 0
    Call RLShowPath
    !insertmacro RLTEXT $0 280 360 160 22 "Change location" 34F2E8 $RLFontB 1
    ${NSD_OnClick} $0 RLPick
  ${Else}
    ; An update or a repair goes where it already is; Uninstall is under it.
    !insertmacro RLTEXT $RLPathLbl 60 336 600 20 "Installed in $INSTDIR" A9B4C4 $RLFont 0
    !insertmacro RLTEXT $0 260 360 200 22 "Uninstall RecompLauncher" 34F2E8 $RLFontB 1
    ${NSD_OnClick} $0 RLUninstall
  ${EndIf}
  nsDialogs::Show
FunctionEnd

; The uninstaller that's there, then this installer closes.
Function RLUninstall
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "UninstallString"
  ${If} $0 == ""
    StrCpy $0 '"$INSTDIR\uninstall.exe"'
  ${EndIf}
  Exec $0
  Call RLQuit
FunctionEnd

; A launcher from 0.8.1 on closes itself when asked (it may be by the clock), instead of the
; installer's "it's running" question. Older ones don't know how, so they get the question.
!macro RLASKQUIT
  nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
  Pop $R0
  ${If} $R0 = 0
  ${AndIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --quit'
    Sleep 800
  ${EndIf}
!macroend

Function RLWelcomeLeave
  ${If} $RLHad != ""
    ${VersionCompare} $RLHad "0.8.1" $0
    ${If} $0 != 2
      !insertmacro RLASKQUIT
    ${EndIf}
  ${EndIf}
FunctionEnd

; 2. Installing. The installer's own page stays hidden and works underneath; on top, on the window
; itself, the picture, a line saying what's happening (set by the RLStep sections) and a bar that
; keeps moving. Taken away again when the Done screen comes.
Var RLPic
Var RLStatus
Var RLBar
!macro RLINSTSHOW UN PIC TEXT
Function ${UN}RLInstShow
  Call ${UN}RLHideButtons
  FindWindow $9 "#32770" "" $HWNDPARENT
  ; Out of the way (the installer shows it again after this, so it's moved rather than hidden).
  System::Call 'user32::SetWindowPos(p r9, p 1, i -30000, i -30000, i 1, i 1, i 0x10)'
  ; The page's placeholder too, so the picture isn't clipped by it.
  GetDlgItem $0 $HWNDPARENT 1018
  ShowWindow $0 ${SW_HIDE}
  !insertmacro RLPX $5 ${RL_W}
  !insertmacro RLPX $6 ${RL_H}
  ; the words
  !insertmacro RLPX $1 60
  !insertmacro RLPX $2 228
  !insertmacro RLPX $3 600
  !insertmacro RLPX $4 22
  System::Call 'user32::CreateWindowEx(i 0, t "STATIC", t "${TEXT}", i 0x50008001, i r1, i r2, i r3, i r4, p $HWNDPARENT, p 0, p 0, p 0) p.r0'
  StrCpy $RLStatus $0
  SetCtlColors $RLStatus 8B97AA ${RL_BG}
  SendMessage $RLStatus ${WM_SETFONT} $RLFont 1
  ; the bar: flat cyan on a dark track, always moving
  !insertmacro RLPX $1 220
  !insertmacro RLPX $2 268
  !insertmacro RLPX $3 280
  !insertmacro RLPX $4 6
  System::Call 'user32::CreateWindowEx(i 0, t "msctls_progress32", t "", i 0x50000009, i r1, i r2, i r3, i r4, p $HWNDPARENT, p 0, p 0, p 0) p.r0'
  StrCpy $RLBar $0
  System::Call 'uxtheme::SetWindowTheme(p r0, w "", w "")'
  SendMessage $RLBar 0x409 0 0x00E8F234
  SendMessage $RLBar 0x2001 0 0x00422B1B
  SendMessage $RLBar 0x40A 1 25
  ; the picture, under both
  System::Call 'user32::CreateWindowEx(i 0, t "STATIC", t "", i 0x5400000E, i 0, i 0, i r5, i r6, p $HWNDPARENT, p 0, p 0, p 0) p.r0'
  StrCpy $RLPic $0
  System::Call 'user32::LoadImage(p 0, t "$PLUGINSDIR\${PIC}", i 0, i 0, i 0, i 0x10) p.r1'
  SendMessage $RLPic 0x172 0 $1
  System::Call 'user32::SetWindowPos(p $RLPic, p 1, i 0, i 0, i 0, i 0, i 0x13)'
  System::Call 'user32::SetWindowPos(p $RLStatus, p 0, i 0, i 0, i 0, i 0, i 0x13)'
  System::Call 'user32::SetWindowPos(p $RLBar, p 0, i 0, i 0, i 0, i 0, i 0x13)'
FunctionEnd

!macroend

!macro RLINSTGONE UN
Function ${UN}RLInstGone
  ${If} $RLPic != ""
    System::Call 'user32::DestroyWindow(p $RLBar)'
    System::Call 'user32::DestroyWindow(p $RLStatus)'
    System::Call 'user32::DestroyWindow(p $RLPic)'
    StrCpy $RLPic ""
  ${EndIf}
FunctionEnd
!macroend

!insertmacro RLINSTSHOW "" rl-progress.bmp "Getting ready…"
!insertmacro RLINSTSHOW "un." rl-unprogress.bmp "Removing RecompLauncher"
!insertmacro RLINSTGONE ""
!insertmacro RLINSTGONE "un."

!macro RLSAY TEXT
  ${If} $RLStatus != ""
    SendMessage $RLStatus ${WM_SETTEXT} 0 "STR:${TEXT}"
    Push $0
    Call RLHideButtons
    Pop $0
  ${EndIf}
!macroend

; 3. Done: open it, or close.
Function RLOpen
  StrCpy $RLRunIt 1
  Call RLNext
FunctionEnd

Function RLDone
  Call RLInstGone
  ${If} $PassiveMode = 1
    ; An update from inside RecompLauncher (/P /R): Tauri's .onInstSuccess opens it again.
    Abort
  ${EndIf}
  ; Launchers have a desktop shortcut: made on a new install (an update keeps what's there).
  Call CreateOrUpdateDesktopShortcut
  Call RLHideButtons
  nsDialogs::Create 1018
  Pop $RLDlg
  SetCtlColors $RLDlg "" ${RL_BG}
  !insertmacro RLBACK $RLDlg rl-done.bmp 0x5000000E
  !insertmacro RLHOT 210 252 300 64 RLOpen
  !insertmacro RLHOT 668 6 44 44 RLNext
  !insertmacro RLTEXT $0 300 336 120 22 "Close" 8B97AA $RLFontB 1
  ${NSD_OnClick} $0 RLNext
  nsDialogs::Show
FunctionEnd

Function RLDoneLeave
  ${If} $RLRunIt == 1
    Call RunMainBinary
  ${EndIf}
FunctionEnd
