# Contributing

Thanks for wanting to help. Bug fixes, translations, docs and new features are all welcome.

If you're planning something big, open an issue or ask in the [Discord](https://discord.gg/FUzqGM2cvf) first so we can agree on it before you put the time in.

## Getting set up

You need Rust 1.90 or newer, Node.js 20 or newer and CMake. The full list for each system, including the Windows and Linux extras, is in [docs/BUILDING.md](docs/BUILDING.md).

```
npm ci
npm run dev        # run the launcher
npm test           # the Rust tests
node --test tests/*.test.cjs   # the UI tests
```

## Pull requests

- Branch off `main` and keep each pull request to one change.
- Run both test commands before you push. Every pull request also builds and tests on Windows, Linux and Mac, and those checks have to pass.
- Say what you changed and why.
- A big change to what players see, like a new or redesigned screen, window, row, button or layout, needs screenshots or a short clip on the pull request: each new or changed screen on the desktop and in Big Picture, and in the light theme where it changes too. Small tweaks like a word, a colour or a spacing fix don't. A big UI change isn't reviewed until the screenshots are there. The maintainers' own pull requests can skip this.
- Don't bump the version or add patch notes. Releases and version numbers are handled separately.
- Merges into `main` need a review from the maintainer.

## How the code is written

- Follow what's already there before you add something new.
- Comments explain why, in plain words, not what the code already says.
- Keep it simple and readable. Early returns over nested conditions, and an extra named variable over a line that runs off the screen.
- Tests should be simple and explicit, one thing per test.
- Headings, buttons and labels in the launcher use Title Case.

## Fixes for one recomp go in that recomp

The launcher is fixed for the launcher's own problems, not for the quirks of one recomp it runs. If a recomp has the wrong folder layout in its release, a broken AppImage entry point, an odd file name or anything else that only that recomp does, fix it in the recomp's own project (or ask its developer to), not here. The recomp rises to the standard; the launcher doesn't add special code for a single recomp. [docs/DEVELOPERS.md](docs/DEVELOPERS.md) is the standard, and [docs/ADD-SUPPORT.md](docs/ADD-SUPPORT.md) is how to bring a recomp up to it. Pull requests that add a fix for one recomp won't be merged.

If many recomps hit the same problem and the guide doesn't cover it, open an issue first, so the standard can be changed for everyone.

## Changing what developers can write

If your change adds or changes something a developer can put in `recomplauncher.json`, update [docs/recomplauncher.schema.json](docs/recomplauncher.schema.json) and the matching guide ([docs/DEVELOPERS.md](docs/DEVELOPERS.md) or [docs/SETUP-INPUTS.md](docs/SETUP-INPUTS.md)) in the same pull request, with a small example. `cargo test` fails when the schema and the code don't match.

## Things the launcher won't do

RecompLauncher never downloads game files, BIOS files, firmware or keys. Pull requests that add that won't be merged. Players always bring their own copy of the game.

## Security issues

Please don't open a public issue for a security problem. See [SECURITY.md](SECURITY.md).

## License

By contributing, you agree your work is released under the [GPL v3](LICENSE), like the rest of the launcher.
