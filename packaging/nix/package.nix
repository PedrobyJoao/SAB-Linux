{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  gtk4-layer-shell,
  libcanberra,
  wayland,
}:

let
  manifest = builtins.fromTOML (builtins.readFile ../../Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = "hyprbreathe";
  inherit (manifest.package) version;

  src = ../..;
  cargoLock.lockFile = ../../Cargo.lock;

  strictDeps = true;
  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
  ];
  buildInputs = [
    gtk4
    gtk4-layer-shell
    libcanberra
    wayland
  ];

  cargoBuildFlags = [
    "-p"
    "hyprbreathe"
  ];
  cargoTestFlags = [ "--workspace" ];

  postInstall = ''
    install -Dm644 crates/platform-linux/assets/*.oga -t "$out/share/hyprbreathe"
    install -Dm644 packaging/io.github.PedrobyJoao.hyprbreathe.settings.desktop \
      "$out/share/applications/io.github.PedrobyJoao.hyprbreathe.settings.desktop"
    install -Dm644 config.example.toml "$out/share/doc/hyprbreathe/config.example.toml"
    install -Dm644 README.md "$out/share/doc/hyprbreathe/README.md"
    install -Dm644 LICENSE "$out/share/licenses/hyprbreathe/LICENSE"
    install -Dm644 THIRD_PARTY_NOTICES.md \
      "$out/share/licenses/hyprbreathe/THIRD_PARTY_NOTICES.md"

    install -Dm644 packaging/systemd/hyprbreathe.service \
      "$out/lib/systemd/user/hyprbreathe.service"
    substituteInPlace "$out/lib/systemd/user/hyprbreathe.service" \
      --replace-fail /usr/bin/hyprbreathe "$out/bin/hyprbreathe"
  '';

  meta = {
    description = "Wayland-native break reminder with multi-monitor overlays";
    homepage = "https://github.com/PedrobyJoao/hyprbreathe";
    license = [
      lib.licenses.mit
      lib.licenses.bsd2
    ];
    mainProgram = "hyprbreathe";
    platforms = lib.platforms.linux;
  };
}
