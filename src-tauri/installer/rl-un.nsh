; The uninstaller's screens, in the installer's look (rl-ui.nsh has the shared parts): are you sure,
; with a tick for also deleting the launcher's settings and game list; removing; then done. The game
; folders, and the saves in them, are never touched.

Var RLChk
Var RLChkOn
Var RLChkOff

Function un.RLTick
  ${If} $DeleteAppDataCheckboxState = 1
    StrCpy $DeleteAppDataCheckboxState 0
    SendMessage $RLChk 0x172 0 $RLChkOff
  ${Else}
    StrCpy $DeleteAppDataCheckboxState 1
    SendMessage $RLChk 0x172 0 $RLChkOn
  ${EndIf}
FunctionEnd

; 1. Uninstall RecompLauncher? Keep it, or Uninstall.
Function un.RLConfirm
  ${If} $PassiveMode = 1
    Abort
  ${EndIf}
  StrCpy $DeleteAppDataCheckboxState 0
  Call un.RLHideButtons
  nsDialogs::Create 1018
  Pop $RLDlg
  SetCtlColors $RLDlg "" ${RL_BG}
  !insertmacro RLBACK $RLDlg rl-unconfirm.bmp 0x5000000E
  !insertmacro RLHOT 176 258 176 52 un.RLQuit
  !insertmacro RLHOT 368 258 176 52 un.RLNext
  !insertmacro RLHOT 668 6 44 44 un.RLQuit
  ; The tick box and its words, under the buttons; either one ticks it.
  System::Call 'user32::LoadImage(p 0, t "$PLUGINSDIR\rl-chk-off.bmp", i 0, i 0, i 0, i 0x10) p.s'
  Pop $RLChkOff
  System::Call 'user32::LoadImage(p 0, t "$PLUGINSDIR\rl-chk-on.bmp", i 0, i 0, i 0, i 0x10) p.s'
  Pop $RLChkOn
  !insertmacro RLPX $1 240
  !insertmacro RLPX $2 345
  !insertmacro RLPX $3 20
  nsDialogs::CreateControl STATIC 0x5000010E 0 $1 $2 $3 $3 ""
  Pop $RLChk
  SendMessage $RLChk 0x172 0 $RLChkOff
  ${NSD_OnClick} $RLChk un.RLTick
  !insertmacro RLPX $1 270
  !insertmacro RLPX $2 342
  !insertmacro RLPX $3 260
  !insertmacro RLPX $4 26
  nsDialogs::CreateControl STATIC 0x50000380 0 $1 $2 $3 $4 "Also delete its settings and game list"
  Pop $0
  SetCtlColors $0 A9B4C4 ${RL_BG}
  SendMessage $0 ${WM_SETFONT} $RLFont 1
  ${NSD_OnClick} $0 un.RLTick
  nsDialogs::Show
FunctionEnd

Function un.RLConfirmLeave
  ; A launcher that's running (maybe by the clock) closes itself first.
  nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
  Pop $R0
  ${If} $R0 = 0
  ${AndIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --quit'
    Sleep 800
  ${EndIf}
FunctionEnd

; 3. Done.
Function un.RLBye
  Call un.RLInstGone
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    Abort
  ${EndIf}
  Call un.RLHideButtons
  nsDialogs::Create 1018
  Pop $RLDlg
  SetCtlColors $RLDlg "" ${RL_BG}
  !insertmacro RLBACK $RLDlg rl-unbye.bmp 0x5000000E
  !insertmacro RLHOT 272 258 176 52 un.RLNext
  !insertmacro RLHOT 668 6 44 44 un.RLNext
  nsDialogs::Show
FunctionEnd
