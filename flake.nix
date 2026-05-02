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
        in
        {
          _module.args.pkgs = import nixpkgs {
            inherit system;
            overlays = [ fenix.overlays.default ];
          };
          packages.default =
            let
              craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
            in
            craneLib.buildPackage {
              src = ./.;
            };
          devShells.default = pkgs.mkShell {
            packages = [
              toolchain
              pkgs.nixd
            ];
          };
        };
    };
}
