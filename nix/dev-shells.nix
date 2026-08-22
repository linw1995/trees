{
  pkgs,
  rustDevToolchainFor,
}: {
  default = pkgs.mkShell {
    nativeBuildInputs = [
      (rustDevToolchainFor pkgs)
    ];
    packages = with pkgs; [
      git
      harper
      prek
    ];
  };
}
