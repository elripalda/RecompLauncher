# Contributing

Thanks for wanting to help. Bug fixes, translations, docs and new features are all welcome.

If you're planning something big, open an issue or ask in the [Discord](https://discord.gg/FUzqGM2cvf) first so we can agree on it before you put the time in.

## Getting set up

You need Rust 1.85 or newer, Node.js 20 or newer and CMake. The full list for each system, including the Windows and Linux extras, is in [docs/BUILDING.md](docs/BUILDING.md).

```
npm ci
npm run dev        # run the launcher
npm test           # the Rust tests
node --test tests/*.test.cjs   # the UI tests
```

## Pull requests

- Branch off `main` and keep each pull request to one change.
- Run both test commands before you push. Every pull request also builds and tests on Windows, Linux and Mac, and those checks have to pass.
- Say what you changed and why, and add a screenshot or clip for anything you can see.
- Don't bump the version or add patch notes. Releases and version numbers are handled separately.
- Merges into `main` need a review from the maintainer.

## How the code is written

- Follow what's already there before you add something new.
- Comments explain why, in plain words, not what the code already says.
- Keep it simple and readable. Early returns over nested conditions, and an extra named variable over a line that runs off the screen.
- Tests should be simple and explicit, one thing per test.
- Headings, buttons and labels in the launcher use Title Case.

## Changing what developers can write

If your change adds or changes something a developer can put in `recomplauncher.json`, update [docs/recomplauncher.schema.json](docs/recomplauncher.schema.json) and the matching guide ([docs/DEVELOPERS.md](docs/DEVELOPERS.md) or [docs/SETUP-INPUTS.md](docs/SETUP-INPUTS.md)) in the same pull request, with a small example. `cargo test` fails when the schema and the code don't match.

## Things the launcher won't do

RecompLauncher never downloads game files, BIOS files, firmware or keys. Pull requests that add that won't be merged. Players always bring their own copy of the game.

## Security issues

Please don't open a public issue for a security problem. See [SECURITY.md](SECURITY.md).

## License

By contributing, you agree your work is released under the [GPL v3](LICENSE), like the rest of the launcher.
