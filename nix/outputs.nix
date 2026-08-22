{
  inputs,
  root,
  description,
}: let
  rust = import ./rust.nix;
  version = (builtins.fromTOML (builtins.readFile (root + /Cargo.toml))).package.version;
  mkPackageSet = import ./packages.nix {
    inherit inputs root description version;
    rustToolchainFor = rust.toolchainFor;
  };
in
  inputs.utils.lib.eachDefaultSystem (
    system: let
      pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [inputs.fenix.overlays.default];
      };
      packageSet = mkPackageSet system;
    in {
      packages = rec {
        default = trees;
        trees = packageSet.trees;
      };

      apps = {
        default = {
          type = "app";
          program = "${packageSet.trees}/bin/trees";
          meta = {inherit description;};
        };
      };

      devShells = import ./dev-shells.nix {
        inherit pkgs;
        rustDevToolchainFor = rust.devToolchainFor;
      };
    }
  )
