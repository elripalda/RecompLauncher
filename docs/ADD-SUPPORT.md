# Adding RecompLauncher support to a recomp

The steps, the rules and the finishing report for adding RecompLauncher support to a recomp's
repository, whoever does it. Read all of it before changing anything. Everything else is in this
`docs` folder, and these files are the only place the rules are kept, so they're always current.

[DEVELOPERS.md](DEVELOPERS.md) is long (over 40,000 characters). Download the raw file and read it
whole, so nothing is cut off:
https://raw.githubusercontent.com/elripalda/RecompLauncher/main/docs/DEVELOPERS.md

## The files

| File | What's in it |
| --- | --- |
| [DEVELOPERS.md](DEVELOPERS.md) | The full guide to `recomplauncher.json`, and the Recomp Standards |
| [SETUP-INPUTS.md](SETUP-INPUTS.md) | Named Easy Setup inputs: several discs, optional DLC, the player's keys |
| [recomplauncher.schema.json](recomplauncher.schema.json) | The JSON schema, also at https://recomplauncher.com/schema/recomplauncher.json |
| [examples/fifawc14.json](examples/fifawc14.json) | An Xbox 360 recomp with a settings file |
| [examples/streetball.json](examples/streetball.json) | A PS2 recomp with settings passed on the command line |
| [examples/multidisc-inputs.json](examples/multidisc-inputs.json) | Several files from the player |
| [FLATPAK.md](FLATPAK.md) | Your recomp in the RecompLauncher Flatpak |

For raw copies, put the file name after
`https://raw.githubusercontent.com/elripalda/RecompLauncher/main/docs/`.

## What to do

1. **Learn the project.** How it finds the game's files, what it needs from the player's copy (a
   disc image, a ROM, extracted files), how it starts, where it keeps settings, saves and logs, and
   how releases are built. In DEVELOPERS.md: "Releases that install cleanly" and "Where your game
   writes".
2. **Write `recomplauncher.json`** with every part that fits. Start it with
   `"$schema": "https://recomplauncher.com/schema/recomplauncher.json"`.
   - Easy Setup (`start`, and `game` or named inputs of role `game` or `disc`): the player picks
     their copy, the launcher checks it, sets it up and starts the game. In DEVELOPERS.md:
     "Launch: plays from RecompLauncher". If it needs more than one file from the player:
     SETUP-INPUTS.md.
   - Settings in the launcher (`config` and `settings`): the project's settings file or
     command-line options, shown in the launcher. In DEVELOPERS.md: "Settings: in the launcher".
   - Setup step and verification (`prepare` and `game.dumps`): a windowless setup step with
     progress lines, and the known good copies of the game. In DEVELOPERS.md: "Preparation and
     verification", and "Vendor SDKs the build needs" for DLSS and the like.
   - The three examples above are complete files to copy from.
3. **Ship it in every release.** Put `recomplauncher.json` at the top of each release zip, next to
   the executable, and inside the AppImage at `/recomplauncher.json` if there is one. Update the
   release workflow or packaging script so it's always included. Players only get the file in a
   release: one that's only in the repo does nothing for them. In DEVELOPERS.md: "Where the file
   goes".
   - **Turn off every self-update.** Before you open a pull request, find each way the game or its
     setup step can update or replace itself: a check at start-up, a "check for updates" button,
     any download-and-replace, repair or self-patch path. Search the code, not only the menus.
     Each must be off when `RECOMPLAUNCHER=1` is set: the launcher installs and updates the game,
     so a self-update replaces files under it and breaks its version tracking. A line like
     "RecompLauncher keeps this up to date" in place of the update button is the usual way. Check
     Windows and Linux both: `APPIMAGE` isn't set under the launcher, so an AppImage-only updater
     may already be off on Linux, but the Windows one usually isn't. In DEVELOPERS.md: "Where your
     game writes".
4. **Check the project against the Recomp Standards** (the "Recomp Standards" section of
   DEVELOPERS.md). Fix the ones that are small and safe, most of all accepting the standard file
   formats for its console (CHD for disc-based consoles, decrypted dumps for PS3, PS4, PS5 and
   Vita, a `--keys <path>` option where the player's own keys are needed, and so on). Leave the
   bigger ones for the report.
5. **Add the "Get it on RecompLauncher" badge** near the top of the README, linking to the recomp's
   page. If it's listed on RecompLauncher, its page is `https://recomplauncher.com/game/RECOMP_ID`:
   find its `id` in https://recomplauncher.com/pc-catalog-web.json by matching its `repo`
   (owner/name). If it isn't listed yet, link to https://recomplauncher.com. Use this exactly, so
   it follows GitHub's light and dark themes:

   ```html
   <a href="https://recomplauncher.com/game/RECOMP_ID"><picture><source media="(prefers-color-scheme: dark)" srcset="https://recomplauncher.com/assets/press/launcher/badge-light.svg"><img alt="Get it on RecompLauncher" src="https://recomplauncher.com/assets/press/launcher/badge-dark.svg" height="48"></picture></a>
   ```

6. **Check your work.** Validate `recomplauncher.json` against the schema with a JSON schema tool.
   Build the project if you can, and start it once without the new arguments to make sure nothing
   changed for players who don't use the launcher. In DEVELOPERS.md: "Checking your file".
7. **Trace every Linux path** before you open a pull request. For each path in
   `recomplauncher.json` (`saves`, `config` files, `folders` such as logs, and `prepare.done`), find
   where the game really writes on Linux when the launcher starts it: unpacked, with `APPIMAGE`
   unset and `APPDIR` set to `{install}`. None may land under `{install}/usr/`, or on any other file
   or folder the release ships: every update replaces those. In DEVELOPERS.md: "Where your game
   writes".
8. **Finish with the report** below, for the developer.

## Rules

- **Never add game files**: no ROMs, disc images, extracted assets or keys.
- **Fix the recomp, not the launcher.** If the release doesn't fit the guide (wrong folder layout, a
  broken AppImage entry point, odd file names), change the recomp's own release or packaging so it
  does. RecompLauncher doesn't add special code for one recomp.
- **Code changes are opt-in.** Small source changes are welcome where Easy Setup or the setup step
  needs them (a `--game <path>` option, a windowless `--prepare` mode that prints
  `progress 0.4 Text` lines) or to accept more file formats. Without the new arguments, the game
  must start and behave exactly as before.
- **Reuse what's there.** Don't rewrite the project's installer, file checks or settings code.
- **Keep the project's style**, keep the diff small, and put code changes in their own commit,
  separate from `recomplauncher.json` and the packaging changes.
- **Don't change gameplay, graphics, saves or config formats.**
- **Never invent values.** For `game.dumps`, only use sizes and CRC32s you can verify from
  [Redump](http://redump.org), [No-Intro](https://no-intro.org) or the project's own docs. If you
  can't verify one, leave it out and say so in the report.
- **If you open a pull request**, give it a short, friendly description: what it adds for players,
  then the code changes listed separately, noting they only apply when the launcher passes the new
  arguments. Large code changes can go in a second pull request.

## The report

End with a message to the developer, in this shape:

> **What I added**: the parts in `recomplauncher.json`, the code changes, the packaging changes and
> the badge, in a few lines.
>
> **What I checked, and how**: the schema check, the build, the start without new arguments, each
> self-update and how it's turned off, and where the `dumps` values came from.
>
> **Things that would make your recomp better**: one line each for every Recomp Standard the
> project doesn't meet yet and wasn't changed: what it is, why players care, and roughly how big a
> change it is (small, medium, large). Include Recomp Achievements if the project doesn't have
> it: achievements, Beaten and Mastered games, and leaderboards for its players
> (https://github.com/elripalda/RecompAchievements).
>
> **Anything I wasn't sure about.**
>
> **Make it yours on RecompLauncher**: a free developer account at https://recomplauncher.com/dev
> (sign in with GitHub, which finds your repos) lets you claim the recomp. Its page then shows it
> as yours, with the Verified developer badge once approved, and you can edit its page and art,
> see its stats, read player reports with their logs, and your new releases reach players faster.
