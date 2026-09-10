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
      (root + /about.hbs)
      (root + /about.toml)
      (root + /migrations)
      (root + /scripts/generate-third-party-notices.sh)
    ];
  };
  cargoArgs = {
    pname = "trees";
    inherit version src;
    strictDeps = true;
    nativeBuildInputs = [
      packagePkgs.cargo-about
      packagePkgs.git
    ];
  };
  cargoArtifacts = craneLib.buildDepsOnly cargoArgs;
  trees = craneLib.buildPackage (cargoArgs // {
    inherit cargoArtifacts;
    # Keep source metadata out of the dependency derivation for cache reuse.
    env = {
      GIT_COMMIT_SHA = inputs.self.sourceInfo.rev or (
        lib.removeSuffix "-dirty" (inputs.self.sourceInfo.dirtyRev or "unknown")
      );
      GIT_DIRTY =
        if inputs.self.sourceInfo ? rev
        then "false"
        else if inputs.self.sourceInfo ? dirtyRev
        then "true"
        else "unknown";
      SOURCE_DATE_EPOCH = toString (inputs.self.sourceInfo.lastModified or 0);
    };
    postInstall = ''
      notices="$TMPDIR/trees-third-party-notices.html"
      CARGO_ABOUT_OFFLINE=1 bash scripts/generate-third-party-notices.sh "$notices"
      install -Dm644 LICENSE "$out/share/licenses/trees/LICENSE"
      install -Dm644 "$notices" "$out/share/licenses/trees/THIRD_PARTY_NOTICES.html"
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
