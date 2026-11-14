# RecompLauncher in Steam's runtime environment (the one nixpkgs' Steam and Heroic use): the
# recomps it downloads are built for ordinary Linux (/lib64/ld-linux, /usr/lib), and Windows ones
# start through umu-run and Proton, which expect the same.
#
# FUSE is left out on purpose: setuid fusermount can't work in the sandbox, and without
# libfuse.so.2 the launcher tells AppImages to unpack themselves instead (sysenv::has_fuse2).
{
  steam,
  recomplauncher-unwrapped,
  umu-launcher,
  extraPkgs ? pkgs: [ ],
  extraLibraries ? pkgs: [ ],
  extraEnv ? { },
}:

steam.buildRuntimeEnv {
  pname = "recomplauncher";
  inherit (recomplauncher-unwrapped) version;

  runScript = "recomplauncher";

  extraPkgs =
    pkgs:
    [
      recomplauncher-unwrapped
      # Windows recomps: proton::runner() prefers umu-run when it's on PATH.
      umu-launcher
    ]
    ++ extraPkgs pkgs;

  # What native recomps link against when they don't carry their own copies.
  extraLibraries =
    pkgs:
    with pkgs;
    [
      sdl2-compat
      sdl3
      openal
      alsa-lib
      libpulseaudio
      pipewire
      libx11
      libxext
      libxrandr
      libxi
      libxcursor
      libxinerama
      libxscrnsaver
      libxkbcommon
      wayland
      freetype
      fontconfig
      zlib
      libpng
      bzip2
      zstd
      openssl
      curl
      libsm
      libice
      gtk3
    ]
    ++ extraLibraries pkgs;

  # Game shortcuts and Steam entries start the launcher again: through this wrapper, by the path
  # the PC finds it at (a profile's bin, which survives updates), else this wrapper's own path;
  # never the store path inside. Steam's profile also sets GTK_IM_MODULE=xim for Steam itself:
  # the PC's own input method is kept aside here and put back below.
  extraPreBwrapCmds = ''
    if [ -z "''${RL_START_PATH:-}" ]; then
      RL_START_PATH=$(command -v recomplauncher || printf '%s' "$0")
      case "$RL_START_PATH" in
        /*) ;;
        *) RL_START_PATH="$PWD/$RL_START_PATH" ;;
      esac
      export RL_START_PATH
    fi
    export RL_HOST_GTK_IM_MODULE="''${GTK_IM_MODULE-}"
  '';

  extraProfile = ''
    if [ -n "$RL_HOST_GTK_IM_MODULE" ]; then
      export GTK_IM_MODULE="$RL_HOST_GTK_IM_MODULE"
    else
      unset GTK_IM_MODULE
    fi
    unset RL_HOST_GTK_IM_MODULE
  '';

  inherit extraEnv;

  extraInstallCommands = ''
    mkdir -p $out/share
    ln -s ${recomplauncher-unwrapped}/share/applications $out/share
    ln -s ${recomplauncher-unwrapped}/share/icons $out/share
  '';

  # Games and their AppImages unpack into /tmp; the launcher and the game must see the same one.
  privateTmp = false;

  inherit (recomplauncher-unwrapped) meta;
}
