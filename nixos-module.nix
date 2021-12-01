{ self }:
{ pkgs, config, lib, ... }:

{
  options.services.ifdyndnsd = with lib; {
    enable = mkOption {
      default = false;
      type = types.bool;
    };
    config = mkOption {
      type = types.str;
    };
    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.system}.ifdyndnsd;
    };
    user = mkOption {
      type = types.str;
      default = "ifdyndnsd";
    };
    group = mkOption {
      type = types.str;
      default = "ifdyndnsd";
    };
  };

  config =
    let
      cfg = config.services.ifdyndnsd;
      configFile = builtins.toFile "ifdyndnsd.toml" cfg.config;
    in lib.mkIf cfg.enable {
      users.users.${cfg.user} = {
        isSystemUser = true;
        group = cfg.group;
      };
      users.groups.${cfg.group} = {};

      systemd.services.ifdyndnsd = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Type = "simple";
          ExecStart = "${cfg.package}/bin/ifdyndnsd ${configFile}";
          User = cfg.user;
          Group = cfg.group;
          Restart = "always";
          RestartSec = "1s";
        };
      };
    };
}
