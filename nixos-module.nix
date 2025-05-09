{ pkgs, config, lib, ... }:

let
  cfg = config.services.ifdyndnsd;

  configFileChecked = pkgs.runCommand "ifdyndnsd.conf" {
    preferLocalBuild = true;
    src = cfg.configFile;
  } ''
    ${cfg.package}/bin/ifdyndnsd --test $src
    cp $src $out
  '';

in {
  options.services.ifdyndnsd = with lib; {
    enable = mkOption {
      default = false;
      type = types.bool;
    };
    config.keys = mkOption {
      type = with types; attrsOf (attrsOf str);
      default = {};
    };
    config.a = mkOption {
      type = with types; listOf (attrsOf (either str int));
      default = [];
    };
    config.aaaa = mkOption {
      type = with types; listOf (attrsOf (oneOf [ str int (attrsOf str)]));
      default = [];
    };
    configFile = mkOption {
      type = types.path;
      default = pkgs.writers.writeTOML "ifdyndnsd.toml" cfg.config;
      defaultText = lib.literalExpression ''pkgs.writers.writeTOML "ifdyndnsd.toml" config.services.ifdyndnsd.config;'';
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
        ExecStart = "${cfg.package}/bin/ifdyndnsd ${configFileChecked}";
        User = cfg.user;
        Group = cfg.group;
        Restart = "always";
        RestartSec = "1s";
      };
    };
  };
}
