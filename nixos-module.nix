{ self }:
{ pkgs, config, lib, ... }: {
  options.services.ifdyndnsd = with lib; {
    enable = mkOption {
      default = false;
      type = types.bool;
    };
    config = mkOption {
      type = types.lines;
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
    logLevel = mkOption {
      type = types.enum [ "trace" "debug" "info" "warn" "error" ];
      default = "info";
    };
  };

  config = let
    cfg = config.services.ifdyndnsd;
    configFile = builtins.toFile "ifdyndnsd.toml" cfg.config;
  in lib.mkIf cfg.enable {
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
    };
    users.groups.${cfg.group} = { };

    systemd.services.ifdyndnsd = {
      wantedBy = [ "multi-user.target" ];
      environment.RUST_LOG = "ifdyndnsd=${cfg.logLevel}";
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
