# RecompLauncher 0.75 Beta 1

## New

- Settings > General has a new switch, "Share anonymous usage stats". It sends when the launcher is
  open and which recomps are being played, under a random ID that isn't linked to you or your
  account. It helps show what people play and how the beta is doing.
- Settings > Beta: tell us how the beta is going without leaving the launcher. Say whether
  something broke or you have an idea, which part of the launcher it's about, and how it feels so
  far. Tick one box to add your PC's parts and your installed games. It goes straight to the
  developer. The same page links to the beta's Discord channels for chat, bug reports and ideas.
- Downloads has an Updates tab: every update you've installed, newest first, the launcher's own
  included. Open one to read its patch notes, and every release in between when you skipped a few.
- Verified developers: a game's page shows who made it, with their picture and the Verified
  developer badge, and their recomps carry the badge beside their names everywhere, in the desktop
  view and in Big Picture mode.
- Follow developers: follow one from their game's page and you get a notification when they put
  out a new recomp or a new version. Home gets a row of their recomps.
- Plays great: a new filter in Discover shows only the recomps most players say run well, and
  covers show their score once enough players have said how it runs.
- Two versions of a game installed show as one game in the Library. Its Play button has a menu to
  start either one.
- Steam Deck: setup starts you in Big Picture mode and offers to add RecompLauncher to Steam, so
  it's in Gaming Mode. On any PC, Settings > Big Picture mode can add it to Steam too.
- How it runs: every game's page shows what players say about it, how many say it plays great,
  has problems or won't run, and the newest reports with the graphics card, processor and Windows
  version each was played on. Once you've played, you can add yours from the same card. Big
  Picture mode shows it in short on each game's page, and the versions list shows each version's
  score. If you didn't get to say how a game ran, the launcher asks again a few sessions later.
- Games with more than one recomp show up once, with a count of their versions on the cover. On the
  game's page, the versions button lists the ones made for your computer (who makes each one, what
  it adds, how it plays) and switches between them in place. Each version installs on its own, so
  you can keep a few and try them side by side. In Big Picture mode, press X on a game's page to
  pick a version.
- A Saves page in the sidebar: every game's saves in one place, how many copies are kept, how much
  space they take on this PC and in the cloud, and a button to back one up right away. Pick a game
  to put an older save back.
- The Downloads page shows what's downloading, which updates are ready (with Update all), a history
  of your installs and updates, and totals like how much you've downloaded and your average speed.
- Home has a Recently updated row, in the desktop view and in Big Picture mode, so new versions
  of recomps are easy to spot.
- Settings > Controllers shows what each button does, in the desktop view and in Big Picture mode.
- Check for updates tells you it's checking, then what it found.

## Fixes

- A PlayStation controller with Steam running could press every button twice in Big Picture mode,
  which could open the wrong page, like Screenshots right after picking a version. Each press now
  counts once.
- Recomps made only for Linux or Mac no longer offer to install on Windows, and the other way
  round.
- After signing in from a game's Saves tab, the tab shows your cloud saves straight away.
- A controller press in the launcher can't reach the quick menu behind it any more, so it can't
  close a running game by accident. Presses in the quick menu stay in the quick menu too.
- Pressing Play twice starts the game once. Pressing Play again right after Stop keeps the new
  session working.
- Uninstalling a game you brought over from another folder leaves that folder where it is. Only a
  game's own folder inside your library can be deleted, and shared folders like Downloads or a
  whole drive are never taken for a game's folder.
- Adding a game to Steam leaves your other non-Steam shortcuts alone, even when Steam's list has
  something the launcher can't read. The very first backup of that list is kept.
- Putting back a save never deletes files the launcher hasn't safely copied first.
- A game that's being moved can't be played, updated or uninstalled until the move is done, and the
  move window stays open until it finishes. A move whose copy is complete always counts, even if
  Windows keeps a leftover file in the old folder.
- A stopped download is only carried on from when it's the same file.
- Backups before an update skip big files (your own game files), so they stay small and quick.
- Crash reports send the log the crash screen showed you, from wherever the game keeps it.
- Usage and diagnostics are anonymous, just as Settings says.
- Screenshots go to the game you started last when two are running.

## Looks and feel

- Going back from a game's page eases out of it and back into where you were.
- A game's page puts what it needs to run on the right, next to its details.
- Discover's filters and toggles change the list in place: the games slide to their new spots
  instead of the page redrawing. The same goes for the Library's filters and sort.
- Collapsing and expanding the sidebar slides smoothly, and the page follows its edge. It's
  lighter now, so it stays smooth after long sessions, even on Discover with every game showing.
- The scrollbar fades away when you stop scrolling, comes back as soon as you scroll, and stays
  clear of the window buttons.
- The Console, Genre and Sort menus are new, match the rest of the launcher, and work with the
  keyboard.
- A game you haven't installed leads with what it is and who makes it. What it needs comes after.
- "Pure black" is now called OLED mode.
- The About window has the Discord logo.
- Big Picture mode fills the whole screen on a Steam Deck and other screens that aren't 16:9.
- Setup fits on a 720p screen without scrolling.
- Long game names and long translations no longer run into each other: names get "…", buttons fit
  their words, and dates on a game's page shrink a little instead of getting cut off.
- The download pill leaves room at the bottom of the page, so nothing hides under it.
- "Based on" lines keep their icon next to the text.
- Bigger click targets on the Home slider.
- More of the launcher is translated, including messages that used to show in English.

## For recomp developers

- Settings > Developers is for verified developers. Sign in with your developer account and it
  shows who you're signed in as, next to the developer tools.
- Your recomp can now tell it was started from RecompLauncher, and from where. Every game gets
  `RECOMPLAUNCHER_MODE` (`bigpicture` or `desktop`) and `RECOMPLAUNCHER_PAD` (`xbox` or
  `playstation`), so it can open full screen with controller menus in Big Picture mode and show the
  right buttons from the start.
- `recomplauncher.json` can set environment variables for your game with `start.env`, like SDL
  hints or your own data folder. The Developer tab lists them when you check your file.
- The developer guide has new parts on releases that install cleanly, controllers and Big Picture
  mode, and getting the most out of your recomp.

Thanks for testing. If something breaks, send it from Settings > Beta, or send the log from the
crash screen. Every report helps.
