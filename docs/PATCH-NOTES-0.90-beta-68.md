# RecompLauncher 0.90 Beta 68

**This is a beta.** Things may change or break while we're in beta. Thanks for your patience.

This one is a big rebuild of how games get set up. Easy Setup now works the same way for every game, builds keep up with every new release, and every build shows you exactly where it's at. It's simpler, steadier and a lot harder to get stuck.

**Easy Setup, rebuilt from the ground up**
- **One setup for every game.** Every game asks for its files the same way, with your copy up front and its update, discs, DLC and keys listed under it. Your files are checked once, when you pick them, and a game only counts as set up once its setup actually works. — Improved by @Xarishark ([#429](https://github.com/elripalda/RecompLauncher/pull/429), [#396](https://github.com/elripalda/RecompLauncher/pull/396), [#392](https://github.com/elripalda/RecompLauncher/pull/392)).
- **Fewer ways to get stuck.** A copy that worked before isn't turned away later, new Switch keys don't send other games back through setup, multi-disc games check every disc, and a long build is never cut off for taking its time. — Improved by @Xarishark ([#429](https://github.com/elripalda/RecompLauncher/pull/429)).
- **A build that's stopped partway builds again.** If a build is cancelled, the launcher is closed, or the PC crashes or loses power, the game builds again before it plays and tells you why, instead of starting a half-built game. Closing the launcher stops a build that's running. — Improved by @Xarishark ([#439](https://github.com/elripalda/RecompLauncher/pull/439)).
- **Setup steps for Windows-only games run on Linux and the Steam Deck** through Proton. — Improved by @Xarishark ([#429](https://github.com/elripalda/RecompLauncher/pull/429)).

**Always on the newest build**
- **Every new release builds again,** so updating always gets you the version you updated to. — Improved by @Xarishark ([#429](https://github.com/elripalda/RecompLauncher/pull/429)).
- **Build Again After Updates,** a new switch on each game's Manage tab, builds the game in the background as soon as its update downloads, so it's ready when you press Play. — Improved by @Xarishark ([#429](https://github.com/elripalda/RecompLauncher/pull/429)).
- **Free Up Space** only offers to remove files when the game can still build again without them. — Improved by @Xarishark ([#429](https://github.com/elripalda/RecompLauncher/pull/429)).

**Downloads, redone**
- **One clean list:** what's downloading or building on top, then what's waiting its turn, with Pause, Pause All and ✕ on each. Updates that finished building get a Built pill. — Improved by @Xarishark ([#430](https://github.com/elripalda/RecompLauncher/pull/430), [#431](https://github.com/elripalda/RecompLauncher/pull/431), [#398](https://github.com/elripalda/RecompLauncher/pull/398)).
- **Every build shows up,** including ones started from a game's page and then hidden, on the desktop and in Big Picture. Each one says Building the game with its percentage, and the Downloads badge counts them. — Improved by @Xarishark ([#443](https://github.com/elripalda/RecompLauncher/pull/443), [#436](https://github.com/elripalda/RecompLauncher/pull/436)).
- **Builds waiting their turn start on their own** as soon as the one before them finishes. — Improved by @Xarishark ([#443](https://github.com/elripalda/RecompLauncher/pull/443)).

**Polish across the launcher**
- **Cloud saves are asked about once, for every game,** right after you sign in, instead of before each game's first Play. Change it any time in Settings → Account, or for one game in its Manage tab. — Improved by @Xarishark ([#442](https://github.com/elripalda/RecompLauncher/pull/442)).
- **Recomps show their own project art,** and Show Game Names switches to the game's art. Bring Your Recomps is in Settings. — Improved by @Xarishark ([#400](https://github.com/elripalda/RecompLauncher/pull/400), [#397](https://github.com/elripalda/RecompLauncher/pull/397)).
- **Cleaner game pages:** play times fit their box, labels are tidier, and the title area stays put. Xbox Series games get their own colour, and sponsor and partner spots were improved. — Improved ([#438](https://github.com/elripalda/RecompLauncher/pull/438), [#411](https://github.com/elripalda/RecompLauncher/pull/411), [#413](https://github.com/elripalda/RecompLauncher/pull/413)).

Thanks for testing, and keep the logs coming from the crash window and Settings → Beta. Every one helps make the next beta better.
