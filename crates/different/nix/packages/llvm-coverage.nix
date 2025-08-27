{
  inputs,
  pkgs,
  ...
}: let
  inherit (pkgs) lib;
  crateBuilder = inputs.self.lib.mkCrateBuilder pkgs;
  commonArgs = crateBuilder.commonArgs;
  cargoArtifacts = crateBuilder.cargoArtifacts;
  craneLib = crateBuilder.craneLib;

  craneLibLLvmTools =
    craneLib.overrideToolchain
    (inputs.fenix.packages.${pkgs.system}.complete.withComponents [
      "cargo"
      "llvm-tools"
      "rustc"
    ]);

  llvm-coverage = craneLibLLvmTools.cargoLlvmCov (commonArgs
    // {
      inherit cargoArtifacts;
    });

  darwin = pkgs.writeShellScriptBin "unsupported" ''
    echo 'Darwin not supported for code coverage"
    exit 1
  '';

  package =
    if pkgs.stdenv.isLinux
    then llvm-coverage
    else darwin;
in
  package
