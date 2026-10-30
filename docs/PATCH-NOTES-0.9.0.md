# RecompLauncher 0.9.0

## Accounts

- Sign in with your RecompStore account: the same one as recompstore.com and recomplauncher.com.
  Google, Discord, GitHub or email all work.
- The launcher opens your browser, you press Sign in, and you're in. No password goes through the
  launcher.
- In Big Picture mode, scan a QR code with your phone instead.
- First-time setup has a new step for it, and Settings has a new Account section. A button at the
  bottom of the sidebar shows who's signed in.
- Everything still works without an account.

## Save backups

- After every session, the launcher keeps a copy of the game's saves. The newest ten stay on your PC.
- Each game has a new Saves tab: your copies, where the game saves, Back up now, and Put back to go
  back to an older save. Putting one back keeps what was there as a copy too.
- The launcher works out where a game saves by watching what it writes while you play. Developers can
  also say where with the new `saves` line in recomplauncher.json.

## Cloud saves

- Signed in, each game asks the first time you press Play whether its saves go to your account.
- After you play, its saves go up to your account. The newest five copies of each game are kept, with
  150 MB of space in all.
- Playing on another PC? Its newer save is put in place before the game starts.
- Played on both since they last matched? The launcher asks which save you want, and keeps the other
  one as a copy.
- Your account page on recomplauncher.com shows your PCs and your cloud saves. You can download or
  delete them there, and sign a PC out.

## For recomp developers

- `"saves": ["{local}/My Game/saves"]` in recomplauncher.json says where your game saves. Developer
  tools checks it.

Thanks for testing. If something breaks, send the log from the crash screen or a report from the
game's page; every one helps.
