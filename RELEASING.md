# Releasing RecompLauncher

1. **Bump the version.** Change it in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` (and `Cargo.lock`) and `package.json`, add `docs/PATCH-NOTES-<version>.md`, and push to `main`.
2. **Wait for the run.** The version bump starts **Release builds** on its own: it makes the tag (`v<version>`), builds Windows, Linux and Mac, checks that every file is this version, and puts them in a draft release. (You can also start it from the Actions page with a tag.)
3. **Sign the Windows installer.** Download `RecompLauncher_<version>_x64-setup.exe` from the draft and sign it with the Certum certificate.
4. **Upload it under the same name.** In the draft, delete the unsigned installer and upload the signed one. The file name has to stay exactly the same.
5. **Run Release check.** From the Actions page, run **Release check** with the tag. It has to be green: every file is this version, the installer is this version inside, it's signed, and it isn't another release's file.
6. **Publish.** Paste the patch notes and publish. Release check runs again by itself when the release is published or edited.

Never publish a release whose files carry another version. Launchers would update to the same version again and again.
