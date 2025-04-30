{
  inputs,
  pkgs,
}: let
  crateBuilder = inputs.self.lib.mkCrateBuilder pkgs;
  craneLib = crateBuilder.craneLib;
  lib = crateBuilder.lib;
  commonArgs = crateBuilder.commonArgs;

  treefmt = inputs.self.lib.mkTreefmt pkgs ../treefmt.nix;
  treefmt-bin = treefmt.treefmt-bin;
  treefmt-programs = treefmt.treefmt-programs;

  # Grab cargo, clippy, rustfmt, etc from crane's devShell to put in our own
  craneToolchain = (craneLib.devShell {}).nativeBuildInputs;

  pre-commit-check = inputs.pre-commit-hooks.lib.${pkgs.system}.run {
    src = ../.;
    hooks = {
      treefmt = {
        enable = true;
        package = treefmt-bin;
      };
    };
  };
in
  pkgs.mkShellNoCC {
    packages = with pkgs;
      [
        rust-analyzer
        cargo-limit
        cargo-nextest
        cargo-dist
        cargo-tarpaulin
        cargo-deny
        just
        bacon
        oranda
        vale
        cargo-release
        cargo-diet
      ]
      # Include the extra packages we use to build our crate
      ++ commonArgs.buildInputs
      # Include crane's toolchain (cargo, clippy, rustfmt, etc)
      ++ craneToolchain
      # Include treefmt and formatters
      ++ treefmt-programs
      ++ [treefmt-bin];

    shellHook = ''
      export PRJ_ROOT="$(git rev-parse --show-toplevel)"

      # Create .pre-commit-config.yaml
      ${pre-commit-check.shellHook}
    '';
  }
