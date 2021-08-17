{ pkgs ? import <nixpkgs> {} }:

with pkgs;


rustPlatform.buildRustPackage rec {
  pname = "ifdyndnsd";
  version = "0.0.0";

  src = runCommandNoCCLocal "ifdyndnsd-src" {} ''
    mkdir -p $out
    cp -ar ${./src} $out/src
    cp -a ${./Cargo.toml} $out/Cargo.toml
    cp -a ${./Cargo.lock} $out/Cargo.lock
  '';

  cargoSha256 = "1rdk9rp6hfxn41pv6yxj8gk9s9hf9i48hwglwhms08rsfvrwb1l9";
}
