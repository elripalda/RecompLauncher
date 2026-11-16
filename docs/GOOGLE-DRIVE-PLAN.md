# Google Drive saves completion plan

Finish `feat/google-drive-saves` so players can use their own Google Drive for saves,
without a RecompStore account, from desktop and Big Picture. Preserve local backups,
explicit consent for each game, and the existing RecompStore save behavior.

## Branch baseline

Reconstructed from the branch at `49e81a1`, based on `main` at `b6516ab`:

- `2cbc5cb` adds the Drive provider, OAuth, storage, save integration, UI and documentation.
- `49e81a1` fixes connection and sync races and adds browser smoke checks.
- The working tree was clean before this plan was added.

Existing implementation:

| Area | Files | Behavior already present |
| --- | --- | --- |
| Provider selection | `src-tauri/src/save_provider.rs` | RecompStore default; explicit switch; per-game consent reset; changes blocked while games run or start |
| Google connection | `src-tauri/src/drive.rs`, `secret.rs`, `win.rs` | Browser OAuth with PKCE and loopback callback; optional device flow; refresh; cancellation; separate credentials |
| Drive storage | `src-tauri/src/drive.rs` | Visible `RecompLauncher/Saves` folder; immutable ZIP snapshots; ancestry-based conflicts; resumable upload tickets; retention |
| Save lifecycle | `src-tauri/src/cloud.rs`, `saves.rs` | Check before play; backup/upload after play; retries; provider/account-scoped sync state; archive validation |
| UI | `ui/account.js`, `ui/tv.js` | Connection, provider choice, disconnect, per-game controls, conflict revision, cache refresh |
| Documentation | `docs/GOOGLE-DRIVE.md` and linked help | Player instructions, build variables, storage behavior and manual release checks |

Code being present does not establish that live Google authorization and storage work.
The existing browser check simulates Tauri responses.

## Scope and decisions to preserve

- Players connect an account; the maintainer supplies the OAuth application configuration.
- Request only `drive.file`; saves and credentials go directly to Google.
- RecompStore remains available and remains the default for existing installations.
- Changing provider or active Google account requires enabling games again. Reconnecting
  the same Google account should preserve consent. Existing copies stay where they are.
- Disconnect removes this PC's credentials and stops its Drive sync. It preserves local
  and remote saves. Global Google revocation remains a separate account action.
- Keep the existing archive format, save discovery, managed-input exclusions, 95 MiB
  upload limit, ten local backups and five ordinary remote copies. Preserve unresolved
  branches until resolution. Concurrent saves must not be ordered solely by clocks.
- Desktop and Big Picture must expose equivalent actions. Browser authorization on the
  same PC is the baseline. Device-code sign-in is conditional on an eligible Google client.
- No new manifest fields, shared input types, downloads, provider migration or library
  synchronization through Drive are included in this feature.

Google documents [desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app),
the limited-input flow and its supported scopes in the
[device authorization guide](https://developers.google.com/identity/protocols/oauth2/limited-input-device),
and the narrow file permission in the
[Drive scope guide](https://developers.google.com/workspace/drive/api/guides/api-specific-auth).

## Work order

### 1 Make release configuration concrete

The release workflows currently do not supply the `RL_GOOGLE_*` build variables consumed
by `drive.rs`. As written, those builds leave connection disabled.

- Wire `RL_GOOGLE_CLIENT_ID` and `RL_GOOGLE_CLIENT_SECRET` into the intended release
  builds. Handle the optional device client separately. Keep credential values out of logs.
- Check Windows, Linux and macOS packaging, plus the separate Arch and Flatpak build
  paths. Document which distributions ship Drive configuration and make that choice explicit.
- Add a release configuration check so an intended Drive-enabled release cannot silently
  ship without its client ID. Ordinary contributor builds should still work without it.
- The maintainer configures the Google project, Drive API, branding, consent audience and
  Desktop client. Confirm applicable publishing requirements and connect a real test account.
  Repository inspection cannot establish the current state of that external project.

Completion: a packaged build reports the expected configuration and completes real consent.

### 2 Verify account isolation and connection lifetime

Review `drive.rs`, `save_provider.rs` and their callers together. Add focused regressions
for canceled or superseded connections, disconnect during refresh, reconnecting the same
account, replacing it with another account, and switching providers while sync waits.

Verify that credentials, upload tickets, pending uploads, synced hashes and UI caches cannot
cross account/provider boundaries. Check restart migration from the existing RecompStore
state, grouped installations and games with their own saves. Verify that game startup and
provider changes cannot interleave into an upload to an unintended destination.

Completion: each transition has an explicit expected consent/state result, and stale work
cannot restore credentials or upload under the wrong account.

### 3 Prove interrupted uploads recover

The current tests cover upload offsets and session URL validation, but do not exercise a
complete Drive HTTP exchange. Add a small test transport seam and deterministic local HTTP
tests for the actual upload logic, keeping production destinations fixed.

Cover partial uploads, restart with a saved ticket, lost success responses, expired sessions,
authentication refresh, throttling, server errors and quota exhaustion. Verify idempotent
completion with the allocated file ID and checksum before creating another snapshot.

Session invalidation after upload 4xx responses is fixed for both probes and data PUTs, with
the cleared URL persisted for launcher restarts. Quota and authorization messages and transient
retry classification are preserved. Broader end-to-end upload recovery remains to be tested. See
[Google's upload recovery rules](https://developers.google.com/workspace/drive/api/guides/manage-uploads).

Completion: retry and restart preserve the local copy, eventually complete one intended
snapshot, and expose an actionable state when authorization or storage prevents progress.

### 4 Prove conflicts and restoration preserve saves

Exercise concurrent uploads from two and three clients, equal timestamps, clock skew,
identical contents, and another upload arriving while the conflict prompt is open. Test both
choices and a failed upload of the resolution. Review pruning against concurrent changes,
pagination, missing ancestors and externally modified metadata.

Expand archive regressions where needed for duplicate entries, traversal, symlinks, damaged
contents, wrong game or destination, oversized expansion, and managed-input exclusions.
Check that failed validation leaves live saves untouched and that restoration preserves the
previous local save through the existing backup path.

Completion: divergent saves stay discoverable until resolved; stale choices cannot silently
resolve unseen changes; an invalid archive cannot change live saves.

### 5 Finish desktop and Big Picture behavior

Extend the existing UI checks to cover reconnect-required state, missing configuration,
provider/account changes, cancellation, offline pending uploads, full storage and conflict
resolution. Check keyboard and controller focus, recovery actions and consistent wording.

Verify actual native browser opening and loopback callbacks on Windows and Linux/Steam Deck,
plus macOS if shipped. Test packaged sandbox behavior for supported Linux distributions.
Only exercise phone authorization with an eligible configured device client.

Completion: each failure has a usable recovery action in both interfaces, and Drive saves
work while signed out of RecompStore.

### 6 Validate and prepare the release

Run the Rust and JavaScript suites and the browser smoke tool. Use two real installations
with the same Google account to upload, restore, play offline, reconnect, interrupt uploads,
create conflicts, resolve each choice, change accounts and disconnect. Include damaged ZIPs,
revoked access, full storage and regression checks for RecompStore-only users.

Update `docs/GOOGLE-DRIVE.md`, linked help and the next patch notes draft to match the final
behavior. No Drive player release note is present in the current branch. If implementation
adds any developer-writable manifest field or `FEATURES` value, update the schema, matching
SDK docs with an example, and the separate "For the website (SDK guide)" note in the same
commit, as required by `AGENTS.md`.

Completion: automated checks pass, supported packages contain their intended configuration,
and recorded native/live results establish that saves survive the release scenarios.

## Validation record

- The three review findings are fixed: archive limits apply per save location, upload 4xx
  responses invalidate persisted sessions, and Drive/OAuth use the launcher's dynamic proxy.
- Drive restores also reject non-save entries, installed release files, selected launch programs,
  managed inputs, mismatched file/glob destinations and links in destination or staging paths.
  The complete archive is validated before any backup or live restore begins.
- Regression tests include a launcher-generated backup with 10,001 files in each of two
  locations, independent size budgets, rejected uploads, retained sessions after server errors,
  and proxy changes on the existing Drive client.
- Restore regressions reject forged executable replacements with matching hashes and confirm
  that a later invalid archive entry leaves earlier saves untouched. Valid file, glob and nested
  folder restores still pass; release files, managed inputs and linked staging paths stay intact.
- JavaScript suite: all 271 tests passed after the fixes.
- Rust suite: 684 passed, one ignored. The full suite ran outside the filesystem sandbox
  because an existing profile test creates and removes a temporary home-directory folder.
- Browser smoke check: desktop, connected account and Big Picture checks passed with no
  reported errors. Artifacts from this run: `/tmp/rl-drive-ui-CgbXpt`.
- Native/live Google checks: still required before public release.

Resume with step 1. Keep this plan updated with completed steps, concrete findings and test
results so work can resume from the repository after a context reset.
