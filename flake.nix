{
  description = "RecompLauncher: find, install, update and play recompiled and decompiled PC games";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      # Steam's runtime environment (packaging/nix/fhs.nix) is x86-64 only, like the launcher's Linux releases.
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      overlays.default = final: prev: {
        recomplauncher-unwrapped = final.callPackage ./packaging/nix/package.nix { };
        recomplauncher = final.callPackage ./packaging/nix/fhs.nix { };
      };

      packages.${system} = rec {
        recomplauncher-unwrapped = pkgs.callPackage ./packaging/nix/package.nix { };
        recomplauncher = pkgs.callPackage ./packaging/nix/fhs.nix { inherit recomplauncher-unwrapped; };
        default = recomplauncher;
      };

      # `npm run dev` / `cargo test --manifest-path src-tauri/Cargo.toml` work from here.
      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [ self.packages.${system}.recomplauncher-unwrapped ];
        packages = [
          pkgs.nodejs
          pkgs.rust-analyzer
          pkgs.clippy
          pkgs.rustfmt
        ];
        # GSettings schemas and GIO modules for the dev window (wrapGAppsHook does this for the package).
        shellHook = ''
          export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS"
          export GIO_EXTRA_MODULES="${pkgs.glib-networking}/lib/gio/modules"
        '';
      };

      formatter.${system} = pkgs.nixfmt;
    };
}
