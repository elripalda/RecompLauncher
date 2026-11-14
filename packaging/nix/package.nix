# The launcher itself, built the way the Arch and Flatpak packages build it: cargo with Tauri's
# production protocol, the static UI embedded from ui/, no Node. fhs.nix wraps it so the games it
# downloads (prebuilt for ordinary Linux) can run on NixOS.
{
  lib,
  rustPlatform,
  pkg-config,
  cmake,
  wrapGAppsHook3,
  gtk3,
  webkitgtk_4_1,
  libsoup_3,
  glib-networking,
  gst_all_1,
  libayatana-appindicator,
  udev,
}:

let
  cargoToml = lib.importTOML ../../src-tauri/Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "recomplauncher-unwrapped";
  inherit (cargoToml.package) version;

  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../src-tauri
      ../../ui
    ];
  };

  cargoRoot = "src-tauri";
  buildAndTestSubdir = "src-tauri";
  cargoLock.lockFile = ../../src-tauri/Cargo.lock;

  buildFeatures = [ "tauri/custom-protocol" ];

  nativeBuildInputs = [
    pkg-config
    # SDL 3 (controllers) is compiled from source by sdl3-sys.
    cmake
    wrapGAppsHook3
  ];

  # cmake is only there for sdl3-sys; the package itself is cargo's.
  dontUseCmakeConfigure = true;

  buildInputs = [
    gtk3
    webkitgtk_4_1
    libsoup_3
    # TLS for the covers the window loads over https.
    glib-networking
    # The window's sounds (ui/sfx/*.webm).
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
  ];

  # The tests run in CI on every platform (npm test). In the Nix build sandbox a few of them fail,
  # because they call /bin/sleep and /usr/bin/true, which aren't there.
  doCheck = false;

  postInstall = ''
    # The crate is also a cdylib (for mobile targets); nothing loads it on the desktop.
    rm -r $out/lib
    install -Dm644 src-tauri/linux/recomplauncher.desktop $out/share/applications/recomplauncher.desktop
    install -Dm644 src-tauri/icons/32x32.png $out/share/icons/hicolor/32x32/apps/recomplauncher.png
    install -Dm644 src-tauri/icons/128x128.png $out/share/icons/hicolor/128x128/apps/recomplauncher.png
    install -Dm644 src-tauri/icons/128x128@2x.png $out/share/icons/hicolor/256x256/apps/recomplauncher.png
    install -Dm644 src-tauri/icons/icon.png $out/share/icons/hicolor/512x512/apps/recomplauncher.png
    install -Dm644 src-tauri/icons/icon.svg $out/share/icons/hicolor/scalable/apps/recomplauncher.svg
  '';

  # Loaded with dlopen, so not in the binary's own list: the tray icon's library, and libudev for
  # SDL's controller hotplug. After fixup (RPATH shrinking would drop them), on the real binary
  # behind wrapGAppsHook's wrapper.
  postFixup = ''
    patchelf --add-rpath ${
      lib.makeLibraryPath [
        libayatana-appindicator
        udev
      ]
    } $out/bin/.recomplauncher-wrapped
  '';

  meta = {
    description = "Find, install, update and play recompiled and decompiled PC games";
    homepage = "https://recomplauncher.com";
    license = lib.licenses.gpl3Plus;
    mainProgram = "recomplauncher";
    platforms = [ "x86_64-linux" ];
  };
}
