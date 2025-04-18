{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, utils }:
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

      in rec {
        # `nix build`
        packages.ifdyndnsd = pkgs.rustPlatform.buildRustPackage {
          pname = "ifdyndnsd";
          version = (
            pkgs.lib.importTOML ./Cargo.toml
          ).package.version + "-" + self.lastModifiedDate;
          # Filter src to avoid unnecessary rebuilds
          src = pkgs.runCommand "ifdyndnsd-src" {} ''
            mkdir $out
            ln -s ${./src} $out/src
            ln -s ${./Cargo.toml} $out/Cargo.toml
            ln -s ${./Cargo.lock} $out/Cargo.lock
          '';
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = with pkgs; [ clippy rustfmt ];
          postCheck = ''
              cargo clippy --all --all-features --tests -- \
                              -D clippy::pedantic \
                              -D warnings \
                              -A clippy::await-holding-refcell-ref
              cargo fmt -- --check
          '';
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
