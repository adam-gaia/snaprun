{
  inputs,
  pkgs,
  ...
}: let
  crateBuilder = inputs.self.lib.mkCrateBuilder pkgs;
  commonArgs = crateBuilder.commonArgs;
  craneLib = crateBuilder.craneLib;
  cargoArtifacts = crateBuilder.cargoArtifacts;

  # Run cargo diet
  cargo-diet = craneLib.mkCargoDerivation (commonArgs
    // {
      buildPhaseCargoCommand = "cargo diet";

      inherit cargoArtifacts;

      pnameSuffix = "-diet";
      nativeBuildInputs = (commonArgs.nativeBuildInputs or []) ++ [pkgs.cargo-diet];
    });
in
  cargo-diet
