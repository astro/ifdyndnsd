{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nmattia/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, utils, naersk, fenix, }:
    let
      systems = [
        "i686-linux"
        "x86_64-linux"
        #"armv6l-linux"
        #"armv7l-linux"
        "aarch64-linux"
        #"riscv64-linux"
      ];
    in utils.lib.eachSystem systems (system:
      let
        pkgs = nixpkgs.legacyPackages."${system}";
        rust = fenix.packages.${system}.stable.withComponents [
          "cargo"
          "rustc"
          "rustfmt"
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
          cargoTestCommands = x:
            x ++ [
              # clippy
              ''
                cargo clippy --all --all-features --tests -- \
                              -D clippy::pedantic \
                              -D warnings \
                              -A clippy::await-holding-refcell-ref''
              # rustfmt
              "cargo fmt -- --check"
            ];
        };
        defaultPackage = packages.ifdyndnsd;

        checks = packages;

        # `nix run`
        apps.ifdyndnsd = utils.lib.mkApp { drv = packages.ifdyndnsd; };
        defaultApp = apps.ifdyndnsd;

        # `nix develop`
        devShell = pkgs.mkShell {
          nativeBuildInputs = with defaultPackage;
            nativeBuildInputs ++ buildInputs;
          packages = with pkgs; [ cargo-edit rust-analyzer ];
        };
      }) // {
        overlay = final: prev: { inherit (self.packages.${prev.stdenv.system}) ifdyndnsd; };

        nixosModule = {
          imports = [ ./nixos-module.nix ];

          nixpkgs.overlays = [
            self.overlay
          ];
        };
      };
}
