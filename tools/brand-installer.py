#!/usr/bin/env python3
"""Puts RecompLauncher's own screens (src-tauri/installer/rl-ui.nsh) into the installer script Tauri
wrote for this build, then builds the installer again with makensis.

  python3 tools/brand-installer.py [target dir]   (default: the windows-gnu release build)

Tauri's own pages go: its welcome, the reinstall question, the folder page and the finish page give
way to RecompLauncher's Install / Installing / Open screens. Everything that installs (files, the
uninstaller, WebView2, shortcuts, updates from the launcher) is still Tauri's."""
import glob, json, os, re, shutil, subprocess, sys
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TAURI = os.path.join(ROOT, 'src-tauri')
target = sys.argv[1] if len(sys.argv) > 1 else os.path.join(TAURI, 'target', 'x86_64-pc-windows-gnu', 'release')
nsis = os.path.join(target, 'nsis', 'x64')
src = os.path.join(nsis, 'installer.nsi')
s = open(src, encoding='utf-8-sig').read()

def swap(old, new, count=1):
    global s
    if s.count(old) != count:
        sys.exit(f'brand-installer: expected {count} of {old[:60]!r}, found {s.count(old)}')
    s = s.replace(old, new)

ui = os.path.join(TAURI, 'installer', 'rl-ui.nsh').replace('\\', '/')
bmp = os.path.join(TAURI, 'installer', 'bmp')
# Our screens, after Tauri's variables and before its pages.
swap('; Installer pages, must be ordered as they appear\n',
     f'!define RLBMP "{bmp}"\n!define MUI_CUSTOMFUNCTION_GUIINIT RLGuiInit\n!include "{ui}"\n\n; Installer pages, must be ordered as they appear\n')
# 1. Welcome -> Install / Update
swap('!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive\n!insertmacro MUI_PAGE_WELCOME\n', 'Page custom RLWelcome RLWelcomeLeave\n')
# The reinstall question: an install over an older one just updates it (Tauri's default choice).
swap('Page custom PageReinstall PageLeaveReinstall\n', '')
# The folder page: the folder is on the first screen.
swap('!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive\n!insertmacro MUI_PAGE_DIRECTORY\n', '')
# Installing: our look.
swap('!insertmacro MUI_PAGE_INSTFILES\n', '!define MUI_PAGE_CUSTOMFUNCTION_SHOW RLInstShow\n!insertmacro MUI_PAGE_INSTFILES\n')
# Finish -> Open RecompLauncher
i = s.index('!define MUI_FINISHPAGE_NOAUTOCLOSE')
j = s.index('!insertmacro MUI_PAGE_FINISH\n') + len('!insertmacro MUI_PAGE_FINISH\n')
s = s[:i] + 'Page custom RLDone RLDoneLeave\n' + s[j:]
# Straight on to the Done screen when the files are in.
swap('OutFile "${OUTFILE}"\n', 'OutFile "${OUTFILE}"\nAutoCloseWindow true\n')
# What the Installing screen says, step by step.
swap('Section WebView2\n', 'Section -RLStep1\n  !insertmacro RLSAY "Checking for WebView2"\nSectionEnd\n\nSection WebView2\n')
swap('Section Install\n', 'Section -RLStep2\n  !insertmacro RLSAY "Copying RecompLauncher"\nSectionEnd\n\nSection Install\n')
# WebView2's download (only on PCs that don't have it): without NSISdl's own progress controls,
# which would show over the Installing screen.
swap('        NSISdl::download "', '        NSISdl::download_quiet "')
# The uninstaller: our screens too. Are you sure (with the "also delete" tick), removing, done.
un = os.path.join(TAURI, 'installer', 'rl-un.nsh').replace('\\', '/')
i = s.index('!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ConfirmShow\n')
j = s.index('!insertmacro MUI_UNPAGE_CONFIRM\n') + len('!insertmacro MUI_UNPAGE_CONFIRM\n')
s = s[:i] + f'!define MUI_CUSTOMFUNCTION_UNGUIINIT un.RLGuiInit\n!include "{un}"\nUninstPage custom un.RLConfirm un.RLConfirmLeave\n' + s[j:]
swap('!insertmacro MUI_UNPAGE_INSTFILES\n', '!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.RLInstShow\n!insertmacro MUI_UNPAGE_INSTFILES\nUninstPage custom un.RLBye\n')
# Straight on to the done screen when it's finished.
swap('''  ; Auto close if passive mode or updating
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd''', '''  SetAutoClose true
SectionEnd''')
# Tauri's functions for the pages that went are still in the script; NSIS warns about them, that's all.
out = os.path.join(nsis, 'installer-rl.nsi')
open(out, 'w', encoding='utf-8-sig').write(s)
conf = json.load(open(os.path.join(TAURI, 'tauri.conf.json')))
ver = conf['version']
exe = os.path.join(target, 'bundle', 'nsis', f'RecompLauncher_{ver}_x64-setup.exe')
os.makedirs(os.path.dirname(exe), exist_ok=True)
s2 = re.sub(r'!define OUTFILE "[^"]*"', '!define OUTFILE "' + exe.replace('\\', '/') + '"', open(out, encoding='utf-8-sig').read())
open(out, 'w', encoding='utf-8-sig').write(s2)
r = subprocess.run(['makensis', '-V2', '-INPUTCHARSET', 'UTF8', out], cwd=nsis, capture_output=True, text=True)
print(r.stdout[-3000:], r.stderr[-3000:])
if r.returncode:
    sys.exit('brand-installer: makensis failed')
print('brand-installer:', exe, os.path.getsize(exe))
