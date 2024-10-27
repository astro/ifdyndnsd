{ pkgs, config, lib, ... }: let
  cfg = config.services.ifdyndnsd;
in {
  options.services.ifdyndnsd = with lib; {
    enable = mkOption {
      default = false;
      type = types.bool;
    };
    config = mkOption {
      type = types.lines;
    };
    configFile = mkOption {
      type = types.path;
      default = builtins.toFile "ifdyndnsd.toml" cfg.config;
      defaultText = ''builtins.toFile "ifdyndnsd.toml" cfg.config;'';
    };
    package = mkOption {
      type = types.package;
      default = pkgs.ifdyndnsd;
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

  config = lib.mkIf cfg.enable {
    users.users.${cfg.user} = {
      isSystemUser = true;
      inherit (cfg) group;
    };
    users.groups.${cfg.group} = { };

    systemd.services.ifdyndnsd = {
      wantedBy = [ "multi-user.target" ];
      environment.RUST_LOG = "ifdyndnsd=${cfg.logLevel}";
      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/ifdyndnsd ${cfg.configFile}";
        User = cfg.user;
        Group = cfg.group;
        Restart = "always";
        RestartSec = "1s";
      };
    };
  };
}
