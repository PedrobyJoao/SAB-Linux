{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.hyprbreathe;
  relayCfg = config.services.hyprbreathe-relay;
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.hyprbreathe;
  defaultRelayPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.hyprbreathe-relay;
in
{
  options.services.hyprbreathe = {
    enable = lib.mkEnableOption "the hyprbreathe Wayland break reminder";

    package = lib.mkOption {
      type = lib.types.package;
      default = defaultPackage;
      defaultText = lib.literalExpression "inputs.hyprbreathe.packages.${pkgs.stdenv.hostPlatform.system}.hyprbreathe";
      description = "The hyprbreathe package to run.";
    };
  };

  options.services.hyprbreathe-relay = {
    enable = lib.mkEnableOption "the hyprbreathe co-op relay";

    package = lib.mkOption {
      type = lib.types.package;
      default = defaultRelayPackage;
      defaultText = lib.literalExpression "inputs.hyprbreathe.packages.${pkgs.stdenv.hostPlatform.system}.hyprbreathe-relay";
      description = "The standalone hyprbreathe relay package to run.";
    };

    listen = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:8787";
      description = "Relay listen address. Keep it private and terminate TLS in a reverse proxy.";
    };

    maxRoomSize = lib.mkOption {
      type = lib.types.ints.between 2 64;
      default = 8;
      description = "Maximum number of connections in one co-op room.";
    };

    maxRooms = lib.mkOption {
      type = lib.types.ints.between 1 65536;
      default = 256;
      description = "Maximum number of simultaneously live co-op rooms.";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      environment.systemPackages = [ cfg.package ];

      systemd.user.services.hyprbreathe = {
        description = "Wayland-native break reminder";
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session.target" ];
        wantedBy = [ "graphical-session.target" ];
        environment.GDK_BACKEND = "wayland";
        serviceConfig = {
          Type = "simple";
          ExecStart = "${lib.getExe cfg.package} daemon";
          Restart = "on-failure";
          RestartSec = "2s";
          UMask = "0077";
        };
      };
    })
    (lib.mkIf relayCfg.enable {
      environment.systemPackages = [ relayCfg.package ];

      systemd.services.hyprbreathe-relay = {
        description = "hyprbreathe co-op relay";
        wantedBy = [ "multi-user.target" ];
        after = [ "network.target" ];
        serviceConfig = {
          Type = "simple";
          ExecStart = "${lib.getExe relayCfg.package} --listen ${relayCfg.listen} --max-room-size ${toString relayCfg.maxRoomSize} --max-rooms ${toString relayCfg.maxRooms}";
          Restart = "on-failure";
          RestartSec = "2s";
          DynamicUser = true;
          NoNewPrivileges = true;
          PrivateTmp = true;
          ProtectHome = true;
          ProtectSystem = "strict";
        };
      };
    })
  ];
}
