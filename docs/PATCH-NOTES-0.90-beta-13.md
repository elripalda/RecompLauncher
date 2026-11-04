# RecompLauncher 0.90 Beta 13

- **GE-Proton gets the right one.** The GE-Proton button could grab the version made for ARM computers, and Windows recomps wouldn't start through it. It now always gets the one for your PC, swaps out a wrong one it finds, and doesn't run into GitHub's request limit anymore.
- **Restoring saves leaves the game alone.** Some games download extra parts the first time they start (like Java), and the launcher could mistake those for saves. Restoring a cloud save then broke the game. Those folders are never treated as saves now, and restores don't touch them.
- **Updates that build the game.** Recomps that come as an installer run it again after an update, so you get the new version instead of the old build.
- **Saves move between systems.** Saves kept in your system's own app data folder back up and restore to the right place on Windows, Linux and Steam Deck, memory card files (.gci) included.
- **More easy setup.** One more recomp gets your disc handed to it straight away, with its settings right in the launcher.
- **Starts the right program.** Tools that come with a game aren't mistaken for the game anymore. On Linux only real programs count, even on memory cards where every file looks runnable. Picking a disc image or a zip as the game's program tells you what to pick instead.
- **Play With Proton.** On Linux, a Windows-only recomp offers to turn Proton on (and grabs GE-Proton if you don't have one) instead of saying there's no Linux version. Recomps you add from GitHub work the same way.
- **Visual C++ in one press.** When a game needs Microsoft's Visual C++ runtime, Install It gets it from Microsoft and installs it for you.
- **Search ignores accents.** Typing a name without accents still finds the game.
- **Fewer false offline moments.** One slow answer from the site doesn't put the launcher in offline mode anymore, and when a site doesn't answer, the message says which one.
- **Clearer help.** Compressed disc images the game can't read get flagged when you pick them. Drive errors and full drives during installs say what happened. A Linux build that needs a newer system points you to its Windows version.
- **Game files a folder deeper.** Picking the folder around your game files works in more cases.
- **One clear button in search.** The Discover search box shows just the one.

Thanks to Xarishark for pitching in on this one.

Keep the logs coming from the crash window and Settings → Beta. Every one helps.
