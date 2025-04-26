default:
    @just --list

check:
    cargo lcheck
    cargo lclippy

build:
    cargo lbuild

run:
    RUST_LOG=debug cargo lrun

test: build
    cargo lbuild --tests
    cargo nextest run --all-targets

fmt:
    treefmt

ci-fmt:
    treefmt --ci

ci-check:
    nix flake check

ci-build:
    nix build

ci-cov:
    nix build .#packages.x86_64-linux.pkgs-llvm-coverage

ci: ci-fmt ci-check ci-build ci-cov
