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

  cargoSha256 = "03s09w9n0nwahjv9ksp0hcf8nbl3g74l2bxan9j4ilmg3sasas3l";
}
