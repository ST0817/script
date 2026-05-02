{
  description = "Rust flake template";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    systems.url = "github:nix-systems/default";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    inputs@{
      nixpkgs,
      flake-parts,
      systems,
      fenix,
      crane,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import systems;
      perSystem =
        { pkgs, system, ... }:
        let
          toolchain = pkgs.fenix.fromToolchainFile {
            file = ./rust-toolchain.toml;
            sha256 = "sha256-gh/xTkxKHL4eiRXzWv8KP7vfjSk61Iq48x47BEDFgfk=";
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          src = craneLib.cleanCargoSource ./.;
          libffi = pkgs.libffi;
          commonArgs = {
            inherit src;
            strictDeps = true;
            nativeBuildInputs = with pkgs; [ llvm_21 ];
            buildInputs = [
              libffi
              pkgs.libxml2
              pkgs.zlib
            ];
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in
        rec {
          _module.args.pkgs = import nixpkgs {
            inherit system;
            overlays = [ fenix.overlays.default ];
          };
          packages.default = craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });
          devShells.default =
            let
              devShell = craneLib.devShell.override {
                mkShell = pkgs.mkShell.override { stdenv = pkgs.clangStdenv; };
              };
            in
            devShell {
              inputsFrom = [ packages.default ];
              packages = with pkgs; [
                nixd
                taplo
              ];
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
                libffi
                pkgs.zlib
                pkgs.stdenv.cc.cc.lib
              ];
            };
        };
    };
}
