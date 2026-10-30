# RecompLauncher 0.6.0

Looks: pick how a game is drawn, and see it before you play.

## Looks

- A new Looks tab on every game: Sharp, Vivid, Film, CRT and Studio monitor, with an amount from
  light to strong.
- The preview uses your own latest screenshot, or the game's banner until you've taken one. Drag
  across it to compare with the original.
- CRT is a curved living-room TV with soft scanlines, a phosphor mask and a little glow. Studio
  monitor is flat and sharp, with crisp lines. Both match the console's own line count, 240 for a
  Nintendo 64 game.
- Looks run on ReShade. The launcher downloads it the first time you play with a look and keeps it
  up to date. Nothing is installed into Windows, and the game's folder is left as it was when you
  close the game.
- Press Home while you play to fine-tune a look in ReShade's menu. Your changes stay until you pick
  another look in the launcher.
- In TV mode, Look and Amount are at the top of a game's settings, with a live preview beside them.
- Right-click a game for Looks, and its page shows the look it starts with.

## For recomp developers

- Looks work without any changes to your recomp. The developer guide explains the few cases where
  a `looks` entry in `recomplauncher.json` helps, such as a game that can draw with either Direct3D
  or Vulkan.

Thanks for testing. If a game won't start with a look, or the look doesn't show, send the log from
the crash screen or a report from the game's page; every one helps.
