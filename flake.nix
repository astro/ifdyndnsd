{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nmattia/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, utils, naersk, fenix }:
    utils.lib.eachDefaultSystem (system: let
      pkgs = nixpkgs.legacyPackages."${system}";
      rust = fenix.packages.${system}.stable.withComponents [
        "cargo"
        "rustc"
        "clippy"
      ];

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
        cargoTestCommands = x: x ++ [
          # clippy
          ''cargo clippy --all --all-features --tests -- -D clippy::pedantic -D warnings -A await-holding-refcell-ref -A clippy::cast-possible-truncation''
        ];
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
