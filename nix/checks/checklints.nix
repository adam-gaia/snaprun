{
  inputs,
  pkgs,
  system,
  ...
}:
# TODO: checklints should expose this in its lib?
pkgs.runCommand "checklints" {
  src = inputs.self;
  nativeBuildInputs = [
    inputs.checklints.packages.${system}.default
    inputs.authors.packages.${system}.default
    inputs.toml-path.packages.${system}.default
  ];
} ''
  cd $src
  # Override cache+config dirs for nix sandbox
  CONFIG_DIR=$(mktemp -d)
  CACHE_DIR=$(mktemp -d)
  ${inputs.checklints.packages.${system}.default}/bin/run-checks --no-user-checklists --config-dir "$CONFIG_DIR" --cache-dir "$CACHE_DIR"
  touch $out # Needed so nix thinks this is a valid derivation
''
