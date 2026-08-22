{
  inputs,
  root,
  version,
  description,
  rustToolchainFor,
}: targetSystem: let
  packagePkgs = import inputs.nixpkgs {
    system = targetSystem;
    overlays = [inputs.fenix.overlays.default];
  };
  lib = packagePkgs.lib;
  craneLib = (inputs.crane.mkLib packagePkgs).overrideToolchain (rustToolchainFor packagePkgs);
  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      (craneLib.fileset.commonCargoSources root)
      (root + /migrations)
    ];
  };
  cargoArgs = {
    pname = "trees";
    inherit version src;
    strictDeps = true;
    nativeBuildInputs = [packagePkgs.git];
  };
  cargoArtifacts = craneLib.buildDepsOnly cargoArgs;
  trees = craneLib.buildPackage (cargoArgs // {
    inherit cargoArtifacts;
    meta = {
      inherit description;
      mainProgram = "trees";
    };
  });
in {
  inherit trees;
}
