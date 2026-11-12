# Troubleshooting

## Getting logs

When a game crashes, the launcher shows a crash screen with the game's log. Use its button to send the log. It goes straight to me, and to the developer if it's their game's problem.

If you need the files yourself, they're in the launcher's data folder:

| System | Data folder |
|---|---|
| Windows | `%APPDATA%\com.recomplauncher.app` |
| Linux | `~/.local/share/com.recomplauncher.app` |
| Mac | `~/Library/Application Support/com.recomplauncher.app` |

- `game-output\` has what each game printed while it ran, one file per game.
- `looks\<game>\ReShade.log` is there if you use Looks (screen effects) on Windows.
- Many games also write their own log inside their install folder. Use **Open Its Folder** on the game's page.

## Windows says "Windows protected your PC"

Windows shows this for newer apps. Click **More info**, then **Run anyway**. Each release lists the installer's SHA-256 if you want to check the file.

## Your antivirus blocks the launcher or a game

The launcher is code-signed, and every download comes from the developer's own GitHub. Some antivirus apps flag new or unsigned programs by mistake, especially recomps, which load game code in ways scanners find unusual.

- Make sure you downloaded it from [Releases](https://github.com/elripalda/RecompLauncher/releases) or recomplauncher.com.
- Tell me on Discord which antivirus it was and what it said. A screenshot helps. I report false positives to the antivirus company.

## A game won't start

1. Check the game's page. If it says **Setup Required**, follow the Set Up steps first.
2. Make sure your copy of the game is the version the recomp asks for. The game's page says what it needs under **Before You Play**.
3. Update your graphics driver.
4. Try **Reinstall** from the game's page. Your saves stay.
5. Still stuck? Send the log from the crash screen, or ask in the [Discord](https://discord.gg/FUzqGM2cvf).

## Windows recomps on Linux won't start

Windows-only recomps run through umu-run and GE-Proton on Linux. If the launcher says the Steam Runtime didn't finish downloading, use **Repair Runtime** in Settings and start the game again.

## A download fails or gets stuck

- Check your connection, then pause and resume it from **Downloads**.
- Make sure the drive with your Recomp Library has enough space.
- Every download is checked against GitHub's checksum. If a check fails, the file is thrown away and you can try again.

## The launcher says it's offline

It only needs the internet for the game list, updates, accounts and cloud saves. Installed games still play offline. It goes back online by itself once it can reach recomplauncher.com again.

## Cloud saves aren't syncing

Make sure you're signed in and cloud saves are on for that game in its **Saves** tab. If a sync fails, the launcher tries again by itself and shows when the next try is.

## Resetting the launcher

- **Settings > Start Over > Reset Settings** puts every setting back to how it started. Your games, saves, library folders and pinned games stay.
- From the same place you can run first-time setup again.
- For a completely fresh start, close the launcher and delete its data folder (see the table above). This removes your settings and the launcher's save backups, so copy anything you want to keep first. Games in your Recomp Library aren't touched.

## Reinstalling

Uninstall from Windows Settings (or remove the app on Linux or Mac) and install the latest version from [Releases](https://github.com/elripalda/RecompLauncher/releases). Your data folder and games stay unless you delete them yourself.
