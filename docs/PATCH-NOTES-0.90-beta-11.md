# RecompLauncher 0.90 Beta 11

- **Starts the actual game.** Some downloads come with extra tools like shader builders, translators, compilers or servers, and the launcher would sometimes pick one of those instead of the game. It knows better now, and if it really can't tell, it asks you which program is the game. Games already in your Library get checked again the next time you press Play.
- **AppImages work without FUSE.** On Linux systems missing libfuse2, AppImages now start anyway.
- **Picks the right Proton.** The launcher only uses a Proton built for your processor.
- **Installs hold up better.** If recomplauncher.com is having a moment, installs fall back to GitHub, and then to the release info you already have. If a release's file changed after the launcher read it, it grabs the new one and keeps going.
- **Programs that need admin.** Games and setups that have to run as administrator now ask Windows for permission instead of failing.
- **Discord from shortcuts.** Games started from a desktop shortcut now show up in your Discord status too, as long as the launcher is open.
- **More crash help on Linux.** The crash window spots when Proton is missing something a game needs, or when a game needs a library your system doesn't have, and tells you what to install.
- **Text Size.** Settings → Appearance has a new Text Size option, from 90% up to 150%, to make the whole launcher bigger or smaller. Ctrl with + or - works too.
- **Row arrows respond everywhere.** The left and right arrows on Home and Discover rows now work wherever you click them.

Keep the logs coming from the crash window and Settings → Beta. Every one helps.
