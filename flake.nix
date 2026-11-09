{
  description = "Wayland-native break reminder with multi-monitor overlays";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      packagesFor =
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          hyprbreathe = pkgs.callPackage ./packaging/nix/package.nix { };
          hyprbreathe-relay = pkgs.callPackage ./packaging/nix/relay.nix { };
        };
    in
    {
      packages = forAllSystems (system: {
        inherit (packagesFor system) hyprbreathe hyprbreathe-relay;
        default = self.packages.${system}.hyprbreathe;
      });

      checks = forAllSystems (system: {
        inherit (self.packages.${system}) hyprbreathe hyprbreathe-relay;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.hyprbreathe ];
            packages = with pkgs; [
              cargo
              clippy
              rust-analyzer
              rustc
              rustfmt
            ];
            RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
          };
        }
      );

      overlays.default = final: _prev: {
        hyprbreathe = final.callPackage ./packaging/nix/package.nix { };
        hyprbreathe-relay = final.callPackage ./packaging/nix/relay.nix { };
      };

      nixosModules = {
        hyprbreathe = import ./packaging/nix/module.nix { inherit self; };
        default = self.nixosModules.hyprbreathe;
      };

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
