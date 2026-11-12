# FAQ

## What's a recomp?

A recomp is a native PC version of a console game. Its developer rebuilds the original game's code so it runs directly on your PC, with no emulator in between. That usually means higher frame rates, widescreen, mods and full controller support.

## Is this legal?

RecompLauncher doesn't include any games or game files, and it never downloads them. Every recomp comes from its developer's own GitHub, and you set it up with your own copy of the game, from a disc or a digital purchase you own.

Some recomps also need keys or other files from your own console. The launcher never downloads those either. You supply them from hardware you own.

## Does it cost anything?

No. The launcher is free and open source under the GPL v3, and so is every recomp listed on it. Accounts are free and optional.

## What systems does it run on?

- **Windows** 10 or 11, 64-bit
- **Linux**, including the Steam Deck. Windows-only recomps can run through umu-run and GE-Proton, which the launcher can set up for you.
- **Mac** with Apple Silicon. This is newer, and Mac versions of recomps depend on each developer.

## Do I need an account?

No. Everything works without one. A free account adds cloud saves, so the same save follows you between your PC, your TV PC and a handheld, and keeps your library in sync.

## Where does it keep my files?

| What | Where |
|---|---|
| Games | `C:\Recomp Library` on Windows, `~/Games/Recomp Library` on Linux and Mac. You can change it or add more folders in Settings. |
| Settings, covers and save backups | The launcher's data folder: `%APPDATA%\com.recomplauncher.app` on Windows, `~/.local/share/com.recomplauncher.app` on Linux, `~/Library/Application Support/com.recomplauncher.app` on Mac |
| Screenshots | `Pictures\RecompLauncher` |

The Windows default is a short folder near the top of the drive because some games break in long or OneDrive-synced paths.

## Will it touch my saves?

It backs up a game's saves before every update. You can also back up, restore and turn on cloud saves per game from the game's **Saves** tab.

## Does it collect data?

Only if you say yes. The first time you open it, the launcher asks whether to share anonymous usage stats: when it's open and which recomps are being played, under a random ID that isn't linked to you. You can change it any time in **Settings > General**.

## Windows says "Windows protected your PC". Is it safe?

Yes. Windows shows that for newer apps it hasn't seen many times yet. The installer is code-signed, and each release lists its file's SHA-256 so you can check you have the real one. Click **More info**, then **Run anyway**.

## A game crashes or looks wrong. Who do I tell?

If it's the game itself, its developer. Every game page links to the developer's GitHub. If it's the launcher (installing, updating, saves, settings), tell me through **Settings > Beta** or [open an issue](https://github.com/elripalda/RecompLauncher/issues/new/choose).

## I make recomps. How do I get mine listed?

Submit it on [recomplauncher.com](https://recomplauncher.com). To get one-click setup, your settings inside the launcher and Big Picture mode support, add a `recomplauncher.json` to your release. The guide is in [DEVELOPERS.md](DEVELOPERS.md).
