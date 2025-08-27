{
  inputs,
  pkgs,
  ...
}: let
  crateBuilder = inputs.self.lib.mkCrateBuilder pkgs;
  commonArgs = crateBuilder.commonArgs;
  cargoArtifacts = crateBuilder.cargoArtifacts;
  craneLib = crateBuilder.craneLib;

  # Build the actual crate itself, reusing the dependency
  # artifacts from above.
  crate = craneLib.buildPackage (commonArgs
    // {
      inherit cargoArtifacts;
      doCheck = false; # Don't run tests as part of the build. We run tests with 'nix flake check'
    });
in
  crate
