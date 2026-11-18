# Nix and NixOS

The Nix package is community-maintained and not officially supported. If you report a problem,
say you use the Nix package.

The repository is a flake (x86_64 Linux). Run the launcher straight from GitHub:

```
nix run github:elripalda/RecompLauncher
```

## Install it on NixOS

Add the flake to your system flake and the package to your system:

```nix
{
  inputs.recomplauncher.url = "github:elripalda/RecompLauncher";

  outputs = { nixpkgs, recomplauncher, ... }: {
    nixosConfigurations.my-pc = nixpkgs.lib.nixosSystem {
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [ recomplauncher.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          # Windows recomps run through Proton, which needs 32-bit graphics drivers too.
          hardware.graphics.enable32Bit = true;
        })
      ];
    };
  };
}
```

`overlays.default` adds `recomplauncher` and `recomplauncher-unwrapped` to nixpkgs instead, for
an overlay-based configuration or Home Manager's `home.packages`.

## What the package is

- `recomplauncher-unwrapped` ([package.nix](package.nix)) is the launcher, built with cargo the
  way the Arch and Flatpak packages build it.
- `recomplauncher` ([fhs.nix](fhs.nix), the default) runs it inside Steam's runtime environment,
  the one nixpkgs' Steam and Heroic use. The recomps it downloads are built for ordinary Linux
  distributions, and they run there unchanged. `umu-run` is in it, so Windows recomps start
  through umu and Proton. `extraPkgs`, `extraLibraries` and `extraEnv` add to it, the same as for
  nixpkgs' `steam`:

  ```nix
  recomplauncher.packages.${pkgs.stdenv.hostPlatform.system}.default.override {
    extraLibraries = pkgs: [ pkgs.libvorbis ];
  }
  ```

- vkBasalt is in it too: the launcher's Looks (shaders) use it on Linux. The layer only turns on
  for a game given a look.
- AppImages unpack themselves to run: FUSE can't mount them inside the runtime environment.
  This applies to both Play and AppImage builders run by Easy Setup.
- RecompLauncher updates through Nix, not by itself. `nix flake update` (or updating the input in
  your system flake) and a rebuild bring the new version.
- Game shortcuts and Steam entries start the launcher by the path your system finds it at (for
  example `/run/current-system/sw/bin/recomplauncher`), so they keep working after an update.

## Building and working on it

```
nix build                     # the launcher, in result/bin/recomplauncher
nix build .#recomplauncher-unwrapped
nix develop                   # Rust, CMake, Node.js and the WebKitGTK libraries
npm run dev                   # inside nix develop
```
