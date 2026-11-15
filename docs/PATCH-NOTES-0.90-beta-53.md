# RecompLauncher 0.90 Beta 53

- **Skip Easy Setup for one game from its menu, or for every game in Settings, and set games up with their own installer or launcher.** — Added by @elripalda ([#215](https://github.com/elripalda/RecompLauncher/issues/215)).
- **Games built from your copy can use a build you already have, wherever it is, with its settings and saves.** — Added by @elripalda ([#215](https://github.com/elripalda/RecompLauncher/issues/215)).
- **Easy Setup shows where a game's own setup builds it.** — Improved by @elripalda.
- **Easy Install lets you choose the exact game folder on desktop and in Big Picture.** Downloads,
  build choices and retries keep that folder, even after restarting the launcher. Updates return
  there. Setup shows separate builder output locations, and installers ask before accepting a
  different folder.

Keep the logs coming from the crash window and Settings → Beta. Every one helps.

## For the website (SDK guide)

- Explain that Easy Install can use an exact player-selected game folder. Show `{install}` in
  setup output, `prepare.done` and launch paths, and explain that builders writing outside the
  install need their own supported destination option; existing external builds stay in place.
