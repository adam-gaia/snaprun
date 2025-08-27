{
  pkgs,
  perSystem,
}:
pkgs.mkShellNoCC {
  packages = [
    perSystem.flake-ci.default
    pkgs.cachix
  ];
}
