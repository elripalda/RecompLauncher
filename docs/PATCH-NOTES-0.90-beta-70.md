# RecompLauncher 0.90 Beta 70

> [!NOTE]
> This is a beta. Things may change or break while we're in beta. Thanks for your patience.

Beta 70 finds Xbox Live Arcade games, lets launch options set variables, shows how long a setup step has left, and gets through networks that block the launcher by name.

## ✨ Highlights

- **Xbox Live Arcade games are found** in your game folders, as Xbox 360 games.
- **Launch options with variables,** like Steam: `NAME=value %command%`.
- **Setup shows the time left** on the step that's running, with **Run in Background**.
- **Gets through networks that block it,** with a new Networking section in Settings.

## 🛠️ Easy Setup and your games

- **Xbox Live Arcade packages are found** as Xbox 360 games, named from the game itself, so their updates and DLC have a game to go with. Thanks @Xarishark ([#638](https://github.com/elripalda/RecompLauncher/pull/638))
- **Setup shows the time left** on the running step, and Run in Background lets you close the window while it carries on. Thanks @Magikdidi24 ([#625](https://github.com/elripalda/RecompLauncher/pull/625))
- **Launch options can set variables,** Steam-style: `NAME=value %command%`. Thanks @Xarishark ([#663](https://github.com/elripalda/RecompLauncher/pull/663))
- A setup step that hands off to a build now waits until the build's finished files appear. Thanks @Xarishark ([#602](https://github.com/elripalda/RecompLauncher/pull/602))
- A DS or GBA cartridge is checked by its game code, so the right copy is matched. Thanks @Xarishark ([#580](https://github.com/elripalda/RecompLauncher/pull/580))
- A `.md` file only counts as a Mega Drive game when it really is one. Thanks @Magikdidi24 ([#631](https://github.com/elripalda/RecompLauncher/pull/631))

## 🌐 Connecting

- **When a network blocks the launcher by name,** like some school and work firewalls, it asks again the way browsers do, so the store still loads. Settings has a new **Networking** section with Hide the Site's Name. Thanks @wyatt-poole ([#248](https://github.com/elripalda/RecompLauncher/pull/248), [#680](https://github.com/elripalda/RecompLauncher/pull/680))

## 🎮 Steam and controllers

- **Steam shortcuts on Windows:** the Steam overlay and Steam Input work even when the launcher is already open. Thanks @Xarishark ([#584](https://github.com/elripalda/RecompLauncher/pull/584))
- X waits for a direction before opening a game's menu, like A already does. Thanks @tiagoalvesber ([#314](https://github.com/elripalda/RecompLauncher/pull/314))

## 🐧 Linux

- AppImages built on Fedora, Bazzite or Silverblue install again. Thanks @Xarishark ([#659](https://github.com/elripalda/RecompLauncher/pull/659))
- Unpacking an AppImage keeps links that point outside the game's folder, and a script named like an AppImage stays a plain file. Thanks @Xarishark and @Piktrip ([#603](https://github.com/elripalda/RecompLauncher/pull/603), [#409](https://github.com/elripalda/RecompLauncher/pull/409))

## 🩺 Crash reports and logs

- Crash reports show your home folder as `~`, so your user name stays private. Thanks @thehobbit85 ([#619](https://github.com/elripalda/RecompLauncher/pull/619))
- Logs with only blank lines are skipped when picking a crash log, and more graphics driver resets are recognised. Thanks @thehobbit85 ([#620](https://github.com/elripalda/RecompLauncher/pull/620), [#621](https://github.com/elripalda/RecompLauncher/pull/621))

## 🎨 Look and feel

- **Star on GitHub:** a button in the sidebar and in Settings → About. Thanks @Xarishark ([#662](https://github.com/elripalda/RecompLauncher/pull/662))
- The newer cloud save badge keeps its answer for the account that asked. Thanks @Xarishark ([#669](https://github.com/elripalda/RecompLauncher/pull/669))
- Portable installs keep letters like é and ü in paths, and words in headings keep their spaces. Thanks @thehobbit85 ([#344](https://github.com/elripalda/RecompLauncher/pull/344), [#618](https://github.com/elripalda/RecompLauncher/pull/618))

## 💙 Thanks

A big thank you to everyone who built this one: @Xarishark, @thehobbit85, @Magikdidi24, @wyatt-poole, @tiagoalvesber and @Piktrip.

Keep the logs coming from the crash window and Settings → Beta. Every one helps make the next beta better.
