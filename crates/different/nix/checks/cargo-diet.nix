{
  inputs,
  pkgs,
  ...
}: let
  crateBuilder = inputs.self.lib.mkCrateBuilder pkgs;
  commonArgs = crateBuilder.commonArgs;
  craneLib = crateBuilder.craneLib;
  cargoArtifacts = crateBuilder.cargoArtifacts;

  # Cargo diet by default exits with 0 when changes are found. This wrapper script changes that
  cargo-diet-wrapped = pkgs.writeShellApplication {
    name = "diet";
    runtimeInputs = [pkgs.cargo-diet];
    text = ''
      [[ "$(cargo diet "$@" | tail -n 1)" == "There would be no change." ]] || exit 1
    '';
  };

  # Run cargo diet
  cargo-diet = craneLib.mkCargoDerivation (commonArgs
    // {
      buildPhaseCargoCommand = "${cargo-diet-wrapped}/bin/diet --dry-run";

      inherit cargoArtifacts;

      pnameSuffix = "-diet";
      nativeBuildInputs = commonArgs.nativeBuildInputs or [];
    });
in
  cargo-diet
