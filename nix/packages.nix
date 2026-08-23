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
      (root + /LICENSE)
      (root + /THIRD_PARTY_NOTICES.html)
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
    postInstall = ''
      install -Dm644 LICENSE "$out/share/licenses/trees/LICENSE"
      install -Dm644 THIRD_PARTY_NOTICES.html "$out/share/licenses/trees/THIRD_PARTY_NOTICES.html"
    '';
    meta = {
      inherit description;
      license = lib.licenses.asl20;
      mainProgram = "trees";
    };
  });
in {
  inherit trees;
}
