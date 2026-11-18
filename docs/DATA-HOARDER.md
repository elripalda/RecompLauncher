# Data Hoarder Mode

Projects get taken down. Data Hoarder Mode keeps your own copy of every release of every game in your
Library, so a recomp you care about outlives its repository.

Turn it on in **Settings → Updates → Data Hoarder**. It is off by default.

## What it does

With the mode on, the launcher downloads **every release** of **every game in your Library**, newest
first and then back through the old ones, and saves each release's description next to its files.

- **Every game in the Library** means games you installed and games you only added with **Add to
  Library**. A game you add while the mode is on starts saving right away.
- **Nothing is installed.** The hoard is a separate folder. Installed games keep working and keep
  updating only through the usual update flow, and the hoarder never touches a game's own folder.
- **Old releases too** (with Releases to keep on All). For a GitHub project with more than 30 releases, the launcher also asks
  GitHub for the older pages (up to 1,000 releases). Those pages are remembered for six hours, so
  adding games or checking for updates doesn't use up GitHub's hourly allowance.
- **Drafts are skipped.** Pre-releases are kept.
- Games added from a local folder (your own builds) have no releases and are skipped.

## Folder layout

```
<hoard folder>/
  <Game>/
    <ReleaseID>/
      RELEASE-NOTES.md     the release's description, with a short header
      release.json         GitHub's own answer for the release, as it came
      Game-Windows.zip     every file the release published
      Game-Linux.tar.gz
```

- `<hoard folder>` is `Releases` inside your default library folder unless you pick another one
  (Settings → Updates → Data Hoarder → Change). It can be on any drive.
- `<Game>` is the game's name, made safe for file names. Two projects with the same name get the game's
  id added to the second one.
- `<ReleaseID>` is the release's tag (`v1.2.0`), made safe for file names.
- `RELEASE-NOTES.md` is rewritten when the developer edits the description, so it stays current.

## Settings

| Setting | Default | What it does |
| --- | --- | --- |
| Data Hoarder Mode | off | Turns the whole thing on. Turning it off stops the sweep at once; the file it was saving keeps its `.part` file and carries on if you turn the mode on again. Files already saved stay. |
| Hoard folder | `<library folder>/Releases` | Where releases are kept. Changing it starts a new sweep into the new folder; the old folder is not moved. |
| Releases to keep | all | **All releases**, **Latest release only**, or **Latest releases, a number** (the newest N, 5 to start). Counted per game, newest first, drafts never counted (a pre-release counts as a release). Narrowing it never deletes anything already saved. |
| Look for New Releases | every hour | How often every Library game is checked by the clock while the launcher is open: every hour, 3, 6, 12 hours, every day or every week. A shorter choice made mid-wait counts from the last sweep. |
| Only builds for this PC | off | Off keeps every platform's files. On keeps only the builds that run on this PC, which takes much less room. |
| Save Now | | Looks for anything new right away. |

## When it looks

A sweep runs when the mode is turned on, when a game is added to the Library, when an install
finishes, when updates are checked, shortly after the launcher starts, and then on the clock
(Look for New Releases: every hour unless you pick a longer gap) while it runs. A sweep that finds everything already saved does very little: it only reads each
game's release list.

## Edge cases

- **Stopped or interrupted downloads.** A file counts as saved only once it has been downloaded whole
  and checked against GitHub's checksum. A stopped download leaves a `.part` file, and the next sweep
  carries on from it.
- **Rate limits.** The launcher reads release lists through recomplauncher.com where it can. If GitHub
  or the site answers with a limit (403, 429) or says it is busy, the sweep stops there and the next
  one tries again, instead of asking again for every game.
- **Not enough space.** The sweep stops with a message when the drive can't hold the next file.
- **Downloads at Once.** Hoarder downloads share the launcher's download slots (Settings → Updates →
  Downloads at Once, one unless you pick more) and its speed limit. A sweep saves one file at a time,
  so with one slot, installing a game while it runs waits at most for the file being saved at that
  moment; with more slots, installs don't wait for it at all.
- **Files taken down.** A file whose download address no longer exists is skipped and tried again on
  later sweeps; nothing else is held up.
- **GitLab and SourceForge.** Their releases come only through recomplauncher.com, so you get what it
  lists. SourceForge files need the checksum the site provides.
- **Files from other places.** A project's files are only downloaded from that project, the same rule as
  installing.

## Not covered

Source code archives (GitHub's "Source code" zip and tarball) are not release files and are not
saved. Cloning the repository is the way to keep the source.
