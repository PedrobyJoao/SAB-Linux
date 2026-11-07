{
  lib,
  rustPlatform,
}:

let
  manifest = builtins.fromTOML (builtins.readFile ../../Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = "hyprbreathe-relay";
  inherit (manifest.package) version;

  src = ../..;
  cargoLock.lockFile = ../../Cargo.lock;

  cargoBuildFlags = [
    "-p"
    "hyprbreathe-relay"
  ];
  cargoTestFlags = [
    "-p"
    "hyprbreathe-relay"
  ];

  meta = {
    description = "Small authenticated WebSocket relay for hyprbreathe co-op rooms";
    homepage = "https://github.com/PedrobyJoao/hyprbreathe";
    license = lib.licenses.mit;
    mainProgram = "hyprbreathe-relay";
    platforms = lib.platforms.linux;
  };
}
