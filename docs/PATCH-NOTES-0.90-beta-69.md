# RecompLauncher 0.90 Beta 69

**This is a beta.** Things may change or break while we're in beta. Thanks for your patience.

This one makes Home yours to follow from the website, brings new consoles, gives Coming Soon games a Save button, and makes setup, saves and downloads steadier all round.

**Home and the store**
- **Home follows the website.** Its rows come in the order set on recomplauncher.com, with info boxes and banners between them, in your language. Big Picture's Home follows the same list ([#462](https://github.com/elripalda/RecompLauncher/pull/462), [#514](https://github.com/elripalda/RecompLauncher/pull/514), [#516](https://github.com/elripalda/RecompLauncher/pull/516)).
- **Big Picture's Home has Favorites and From developers you follow,** like the desktop, and the box to add a folder of your games when none is picked yet ([#568](https://github.com/elripalda/RecompLauncher/pull/568)).
- **New consoles:** Xbox Series X|S, Xbox One, PS Vita, TurboGrafx-16 and Mobile ([#515](https://github.com/elripalda/RecompLauncher/pull/515)).
- **Coming Soon games have one Save button** that shows how many people want it, and you get a notification when it comes out ([#466](https://github.com/elripalda/RecompLauncher/pull/466)).
- **Launcher Support** only counts what the launcher can actually use, and a support file it can't read says why. — Improved by @Xarishark ([#532](https://github.com/elripalda/RecompLauncher/pull/532)).
- **Customize can use your own LaunchBox** for pictures and descriptions, and About the game credits LaunchBox when its info came from there ([#518](https://github.com/elripalda/RecompLauncher/pull/518), [#538](https://github.com/elripalda/RecompLauncher/pull/538)).
- **Reviews:** a one-page review window, Delete Review, All Reviews, and a searchable Your Reviews. All Reviews' score and rows follow the version you pick. — Improved by @Xarishark ([#491](https://github.com/elripalda/RecompLauncher/pull/491), [#566](https://github.com/elripalda/RecompLauncher/pull/566)).
- **Light theme** reads properly over pictures, with a paper-white look and colours that read on it. — Improved by @Xarishark ([#578](https://github.com/elripalda/RecompLauncher/pull/578)).
- Full-width backgrounds keep their full quality, and the Featured Banner is never taller than half the window. — Improved by @thehobbit85 and @Xarishark ([#346](https://github.com/elripalda/RecompLauncher/pull/346), [#513](https://github.com/elripalda/RecompLauncher/pull/513)).

**Easy Setup**
- **A Set Up by You pill** on games you set up yourself, and a Settings switch for Easy Setup on new games, which asks before it turns off. — Improved by @Xarishark ([#526](https://github.com/elripalda/RecompLauncher/pull/526)).
- **When a setup step fails,** you see the reason and its last lines, with Copy and Open Full Log. — Improved by @Xarishark ([#510](https://github.com/elripalda/RecompLauncher/pull/510)).
- Easy Setup says when a game wants a folder, not a file, sets up one game of several by its own id, and Before You Play no longer asks for your copy when the game asks for it itself or while it builds. — Improved by @Xarishark ([#511](https://github.com/elripalda/RecompLauncher/pull/511), [#521](https://github.com/elripalda/RecompLauncher/pull/521), [#508](https://github.com/elripalda/RecompLauncher/pull/508)).
- When the launcher can't tell a game's program from a helper tool, it asks you, and a release's tools are named for what they do. Mac setups ignore hidden metadata files. — Improved by @thehobbit85 and @Xarishark ([#572](https://github.com/elripalda/RecompLauncher/pull/572), [#401](https://github.com/elripalda/RecompLauncher/pull/401)).
- A sequel and its original game are told apart, a sequel's disc doesn't match the earlier game, and Start With offers the game, not its tools. — Improved by @thehobbit85 and @AngeloAscheri ([#479](https://github.com/elripalda/RecompLauncher/pull/479), [#472](https://github.com/elripalda/RecompLauncher/pull/472), [#483](https://github.com/elripalda/RecompLauncher/pull/483)).

**Saves**
- **A cloud save never goes over newer saves** made after the check, the save kept from before a restore never becomes the newest cloud save, and playing without a cloud save that can't be put back asks first. — Improved by @Xarishark and @thehobbit85 ([#528](https://github.com/elripalda/RecompLauncher/pull/528), [#477](https://github.com/elripalda/RecompLauncher/pull/477), [#467](https://github.com/elripalda/RecompLauncher/pull/467)).
- **Cloud save notes belong to the account that made them,** so switching accounts never sends one account's save to another. — Improved by @Xarishark ([#579](https://github.com/elripalda/RecompLauncher/pull/579)).
- **Change the save folder** for games set up without Easy Setup. — Improved by @Xarishark ([#512](https://github.com/elripalda/RecompLauncher/pull/512)).
- **Reset Play Time** from a game's menu and in Big Picture. — Improved by @Xarishark ([#495](https://github.com/elripalda/RecompLauncher/pull/495)).

**Downloads and updates**
- **Keep downloaded files** keeps the zip's GitHub date and installer zips, and Open finds them. — Improved by @Xarishark ([#519](https://github.com/elripalda/RecompLauncher/pull/519)).
- Uninstall shows Uninstalling… while it runs. A download that can't connect names the server it really failed at. — Improved by @Xarishark ([#507](https://github.com/elripalda/RecompLauncher/pull/507), [#458](https://github.com/elripalda/RecompLauncher/pull/458)).
- An update check that finishes after you picked another release no longer installs over your choice, and the update window closes once a download is handed off to your browser. — Improved by @thehobbit85 ([#403](https://github.com/elripalda/RecompLauncher/pull/403), [#404](https://github.com/elripalda/RecompLauncher/pull/404)).
- Uninstalling a game keeps everything else away from its folder while it goes. — Improved by @Xarishark ([#573](https://github.com/elripalda/RecompLauncher/pull/573)).
- Game updates are checked again when a newer catalog arrives, and versions like release candidates are ordered correctly. — Improved by @thehobbit85 ([#476](https://github.com/elripalda/RecompLauncher/pull/476), [#474](https://github.com/elripalda/RecompLauncher/pull/474), [#473](https://github.com/elripalda/RecompLauncher/pull/473), [#475](https://github.com/elripalda/RecompLauncher/pull/475), [#480](https://github.com/elripalda/RecompLauncher/pull/480), [#481](https://github.com/elripalda/RecompLauncher/pull/481)).

**Controllers and Big Picture**
- **Xbox Wireless Adapter:** controllers no longer switch off when they reconnect. — Improved by @Xarishark ([#517](https://github.com/elripalda/RecompLauncher/pull/517)).
- A game's buttons in Big Picture go on to a second line when they don't fit. — Improved by @AngeloAscheri ([#577](https://github.com/elripalda/RecompLauncher/pull/577)).
- Pick a release's build with the controller in Big Picture, turn the controller off outside Big Picture in Settings, and A waits for a direction after the window loses focus. — Improved by @AngeloAscheri and @thehobbit85 ([#469](https://github.com/elripalda/RecompLauncher/pull/469), [#482](https://github.com/elripalda/RecompLauncher/pull/482), [#478](https://github.com/elripalda/RecompLauncher/pull/478)).

**Fixes**
- Pages that load while the launcher is behind another window show right away instead of staying blank until you click it ([#569](https://github.com/elripalda/RecompLauncher/pull/569)).
- Customize's hint before you pick a picture stays inside the frame ([#567](https://github.com/elripalda/RecompLauncher/pull/567)).
- **Vietnamese** is now fully translated, with a Vietnamese README. — Improved by @dung997tb ([#539](https://github.com/elripalda/RecompLauncher/pull/539), [#537](https://github.com/elripalda/RecompLauncher/pull/537), [#575](https://github.com/elripalda/RecompLauncher/pull/575), [#540](https://github.com/elripalda/RecompLauncher/pull/540)).
- Library covers fill their tiles on Windows PCs that drew them blank. — Improved by @Xarishark ([#527](https://github.com/elripalda/RecompLauncher/pull/527)).
- **Export Logs** puts every log in one zip on your desktop (Settings → About → Troubleshoot), and the start failure box only says WebView2 is missing when it is. — Improved by @Xarishark ([#455](https://github.com/elripalda/RecompLauncher/pull/455), [#489](https://github.com/elripalda/RecompLauncher/pull/489), [#505](https://github.com/elripalda/RecompLauncher/pull/505)).
- Linux: unpacked AppImages and programs in subfolders start properly, and a just-unpacked setup program is tried again when Linux says it's busy. — Improved by @Xarishark ([#486](https://github.com/elripalda/RecompLauncher/pull/486), [#494](https://github.com/elripalda/RecompLauncher/pull/494), [#525](https://github.com/elripalda/RecompLauncher/pull/525)).

**For developers**
- A test mode for checking a build with automated tools, off unless the launcher is started with it ([#548](https://github.com/elripalda/RecompLauncher/pull/548), [#565](https://github.com/elripalda/RecompLauncher/pull/565)). Its token isn't passed on to games and tools the launcher starts. — Improved by @Xarishark ([#571](https://github.com/elripalda/RecompLauncher/pull/571)).

Thanks for testing, and keep the logs coming from the crash window and Settings → Beta. Every one helps make the next beta better.
