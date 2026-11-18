# This fork, and how it stays current

This repository is a fork of [elripalda/RecompLauncher](https://github.com/elripalda/RecompLauncher)
("upstream"). It is upstream's launcher with a small stack of the fork's own commits on top:

- **Data Hoarder Mode** (docs/DATA-HOARDER.md): every release of every Library game is downloaded
  and kept in `Releases/<game>/<release>/`, with its notes, without installing anything. Includes
  **Look for New Releases**: how often it checks by the clock (every hour by default).
- **Downloads at Once** and **Unpacks at Once** (Settings → Updates → Downloads): how many downloads,
  and how many unpacks of finished downloads, run at the same time. Unset, the launcher behaves as
  upstream does: one download at a time, unpacks unlimited. `src-tauri/src/slots.rs`.
- **Updates from this fork.** The launcher's own updater reads this repository's Latest release
  (`releases/latest/download/version.json`, set by `FORK_REPO` / `RECOMP_FORK_REPO` in
  `src-tauri/src/lib.rs`), not recomplauncher.com, so an installed fork build only ever updates to
  the next fork build.
- **Update schedules** (Settings → Updates): **Check Games Every** and **Check RecompLauncher
  Every**, hourly by default (1, 3, 6, 12 hours or a day). Each game check installs the games set to
  Update automatically (and Data Hoarder Mode saves what's new); a newer RecompLauncher found by the
  clock is downloaded at once, so Update Now only installs it. The **RecompLauncher Updates** switch
  turns the launcher's own checks off.
- **The upstream sync** (`.github/workflows/fork-sync.yml`) and this page.

Anything upstream already merged (the Auto-Install setting, for one) isn't carried here.

## The layout: fork commits replayed on top of upstream

```
upstream main   ── A ── B ── C ── D ── E          (new upstream commits)
                                         \
fork main                                 P1 ── P2 ── P3 ── P4   (the fork's commits, replayed)
```

Fork `main` is always "upstream main, plus the fork's commits on top". There's no separate
branch to keep in step: the fork's commits are simply whatever fork `main` has that upstream
doesn't.

Once a day (and whenever it's started by hand: **Actions → Sync with upstream → Run workflow**),
the workflow:

1. fetches upstream `main`;
2. replays the fork's commits onto it (`git rebase --onto upstream/main <merge-base> main`);
3. runs upstream's tests and the fork's own (`node --test tests/*.test.cjs`, `cargo test`);
4. only if everything passed, moves fork `main` there. The previous tip is kept as the
   `sync-backup` branch, so nothing is ever lost;
5. builds the Windows installer with their own version (`tools/fork-version.cjs`: upstream
   `0.90.0-beta.64` becomes `0.90.0-beta.64.<run number>`, so every fork build is newer than the
   last) and publishes them as the repository's **Latest** release, tagged `hoarder-v<version>`,
   with a `version.json` the installed launchers read.

If upstream hasn't moved, it does nothing.

### Why replay (rebase) and not merge

- The fork's changes stay a short, readable list of commits on top of upstream. `git log
  upstream/main..main` is always exactly "what this fork adds", which is also what an upstream pull
  request would contain.
- A conflict is met one fork commit at a time, against the newest upstream, and is fixed once.
  With merges, every upstream change is merged into an ever-growing history and the fork's own
  changes get spread across many merge commits, which makes later upstream PRs hard to cut.
- The cost: fork `main` is rewritten on each sync (force-pushed). That's fine for a fork nobody else
  builds on; `sync-backup` and the prerelease tags keep the old tips.

## When the sync stops on a conflict

When upstream changes the same lines a fork commit changes, the replay stops. The workflow then
pushes nothing (fork `main` and the last prerelease stay as they were) and opens an issue labelled
`upstream-sync` naming the fork commit and the files in conflict. The issue is closed by the next
run that goes through.

To fix it by hand:

```sh
git remote add upstream https://github.com/elripalda/RecompLauncher.git   # once
git fetch upstream
git checkout main && git pull
git rebase --onto upstream/main "$(git merge-base main upstream/main)" main
# fix the files it names, then: git add <files> && git rebase --continue (repeat as needed)
node --test tests/*.test.cjs && (cd src-tauri && cargo test)
git push --force-with-lease origin main
```

Then run the workflow by hand to build the prerelease. (Or ask Claude in the project to do the
rebase on a branch and open a PR.)

Most conflicts are in a few shared places: the `Settings` struct in `src-tauri/src/library.rs`
(keep both sides' fields), the `invoke_handler!` list and `save_settings` in `src-tauri/src/lib.rs`,
and the Settings → Updates page in `ui/app.js`. The fork keeps its footprint in upstream files
small on purpose: new code lives in its own files (`hoard.rs`, `slots.rs`), App's upstream
`download_slot` field is left in place (unused) so upstream's code that builds `App` merges
unchanged, and the fork's settings rows are looked up by name so upstream's page tests pass
without edits.

## If upstream takes a feature

Once upstream merges one of the fork's features, the next sync replays the fork's commit for it on
top of upstream's version. If the two are identical, git drops the fork's commit by itself. If
they differ (upstream reworded or reworked it, as happened with Auto-Install), the replay stops on a
conflict: drop the fork's commit for that feature (`git rebase --skip` at that point, or remove it
from the list with an interactive rebase) and keep upstream's version.

## If upstream never takes Data Hoarder Mode

Nothing changes: the fork keeps carrying it. The daily sync keeps replaying it onto each new
upstream, and the prerelease is the build to install. The only ongoing cost is the occasional
conflict above, usually a few lines in the shared places listed. Upstream pull requests are
opened by hand from compare links such as
`https://github.com/elripalda/RecompLauncher/compare/main...nickhighland:RecompLauncher:<branch>?expand=1`.

## One-time setup

1. **Actions on.** Settings → Actions → General → Allow all actions. In a fork, scheduled workflows
   also need the Actions tab's "I understand my workflows, go ahead and enable them".
2. **The sync token.** Create a fine-grained personal access token (GitHub → Settings → Developer
   settings → Fine-grained tokens) for this repository only, with **Contents: Read and write** and
   **Workflows: Read and write**. Save it as the repository secret `FORK_SYNC_TOKEN` (Settings →
   Secrets and variables → Actions). Without it, GitHub refuses any sync that brings in upstream
   changes to `.github/workflows`.
3. **Quiet upstream's own automation.** In the Actions tab, disable the workflows that only make
   sense in upstream: *Release*, *Release check* and *Issue triage*. They'd otherwise start on the
   fork's pushes and issues and fail for lack of upstream's secrets.
4. **Install a fork build once** (from the Latest release). From then on it updates itself from
   this fork. Builds older than this change still read recomplauncher.com: keep RecompLauncher
   Updates off in those until the fork build is installed.

GitHub pauses scheduled workflows in a repository with no activity for 60 days; the sync's own
pushes count as activity while upstream is active. If it ever pauses, the Actions tab has a button
to turn it back on.
