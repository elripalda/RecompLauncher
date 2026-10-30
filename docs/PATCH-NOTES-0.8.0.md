# RecompLauncher 0.8.0

The new look from recomplauncher.com, a proper installer, and tools for recomp developers.

## A new installer

- One small window with the RecompLauncher logo: Install, a progress bar, then Open
  RecompLauncher.
- Change where it installs right on the first screen.
- Running it again over an older version shows Update, and keeps your games, saves and settings.

## The new look

- Midnight blue and cyan, with the same headings, logo and buttons as recomplauncher.com.
- The logo in the sidebar fits when the sidebar is open.
- Games without art, like your own builds, get a short banner instead of an empty one.

## Big Picture mode

- TV mode is now Big Picture mode.
- First-time setup always opens in a window, even if you picked Big Picture mode before.
- Settings in Big Picture mode are in groups, with the one you're on shown large beside the list.

## Settings

- Settings are split into sections down the side: General, Games and folders, Updates and
  downloads, Appearance, Big Picture mode, Controllers, Your PC, Developers and About.

## Fixes

- Looks load properly. Some PCs showed "Looks didn't load" on every game.
- The launcher's languages load the same way, so it shows in your language every time.
- The setup screen has its own minimize, maximize and close buttons.

## For recomp developers

- Turn on Developer tools in Settings, under Developers, and add your recomp from a folder on
  your PC. It shows in your Library as a local build.
- Its Developer tab checks your `recomplauncher.json` every time you press Check again: where Play
  starts, what the setup screen asks for, each settings file and the choice each setting shows.
  JSON mistakes show the line and column.
- No file yet? "Make a starter file" writes one next to your program.
- Try easy setup with your own disc and change your settings there before you ship.
- `name` in `recomplauncher.json` names your local build.

Thanks for testing. If something breaks, send the log from the crash screen or a report from the
game's page; every one helps.
