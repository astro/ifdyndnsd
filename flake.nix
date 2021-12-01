{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nmattia/naersk";
    mozillapkgs.url = "github:mozilla/nixpkgs-mozilla";
    mozillapkgs.flake = false;
  };

  outputs = { self, nixpkgs, utils, naersk, mozillapkgs }:
    utils.lib.eachDefaultSystem (system: let
      pkgs = nixpkgs.legacyPackages."${system}";
      mozilla = pkgs.callPackage (mozillapkgs + "/package-set.nix") {};
      rust = (mozilla.rustChannelOf {
        channel = "stable";
        date = "2021-10-04";
        sha256 = "0swglfa63i14fpgg98agx4b5sz0nckn6phacfy3k6imknsiv8mrg";
      }).rust;

      # Override the version used in naersk
      naersk-lib = naersk.lib."${system}".override {
        cargo = rust;
        rustc = rust;
      };
    in rec {
      # `nix build`
      packages.ifdyndnsd = naersk-lib.buildPackage {
        pname = "ifdyndnsd";
        src = ./.;
      };
      defaultPackage = packages.ifdyndnsd;

      checks = packages;

      # `nix run`
      apps.ifdyndnsd = utils.lib.mkApp {
        drv = packages.ifdyndnsd;
      };
      defaultApp = apps.ifdyndnsd;

      # `nix develop`
      devShell = pkgs.mkShell {
        nativeBuildInputs = with defaultPackage;
          nativeBuildInputs ++ buildInputs;
      };
    }) // {
      overlay = final: prev: {
        ifdyndnsd = self.packages.${prev.system};
      };

      nixosModule = import ./nixos-module.nix { inherit self; };
    };
}
