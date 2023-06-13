{
  description = "runner";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/23.05";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    shell-utils.url = "github:waltermoreira/shell-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
  };
  outputs =
    { self
    , nixpkgs
    , flake-utils
    , crane
    , shell-utils
    , rust-overlay
    }:
      with flake-utils.lib; eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        shell = shell-utils.myShell.${system};
        craneLib = crane.mkLib pkgs;
        src = craneLib.cleanCargoSource (craneLib.path ./.);
        commonArgs = {
          inherit src;
          buildInputs = [
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        runner = craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });
      in
      {
        packages.default = runner;
        devShells.default = shell {
          name = "runner";
          packages = [
            (pkgs.rust-bin.stable.latest.default.override {
              extensions = [ "rust-src" ];
            })
          ];
        };
      });
}
