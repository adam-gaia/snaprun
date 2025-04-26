{inputs, ...}: let
  mkTreefmt = pkgs: treefmtPath: let
    inherit (pkgs) lib;

    # Treefmt doesn't easily expose the programs with out its flake-parts module (as far as I can tell)
    # This snipit, modified from their default.nix, lets us grab the programs after building with our treefmt config
    treefmt-module-builder = nixpkgs: configuration: let
      mod = inputs.treefmt-nix.lib.evalModule nixpkgs configuration;
    in
      mod.config.build;
    treefmt-module = treefmt-module-builder pkgs (import treefmtPath);
    treefmt-bin = treefmt-module.wrapper;
    treefmt-programs = lib.attrValues treefmt-module.programs;
  in {inherit treefmt-bin treefmt-programs;};

  mkCrateBuilder = pkgs: let
    inherit (pkgs) lib;

    craneLib = inputs.crane.mkLib pkgs;
    txtFilter = path: _type: builtins.match ".*txt" path != null;
    markdownFilter = path: _type: builtins.match ".*md" path != null;
    sourceWithReadme = path: type: (markdownFilter path type) || (txtFilter path type) || (craneLib.filterCargoSources path type);
    # nix build needs access to the READAME and test/*.txt files for trycmd tests
    src = lib.cleanSourceWith {
      src = ../../.;
      filter = sourceWithReadme;
      name = "source";
    };

    # Common arguments can be set here to avoid repeating them later
    commonArgs = {
      inherit src;
      strictDeps = true;

      buildInputs =
        [
          # Add additional build inputs here
        ]
        ++ lib.optionals pkgs.stdenv.isDarwin [
          # Additional darwin specific inputs can be set here
          pkgs.libiconv
        ];

      # Additional environment variables can be set directly
      # MY_CUSTOM_VAR = "some value";
    };

    # Build *just* the cargo dependencies, so we can reuse
    # all of that work (e.g. via cachix) when running in CI
    cargoArtifacts = craneLib.buildDepsOnly commonArgs;
  in {
    inherit src lib craneLib commonArgs cargoArtifacts;
  };
in {
  inherit mkCrateBuilder mkTreefmt;
}
