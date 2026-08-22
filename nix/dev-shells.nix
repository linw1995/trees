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
      diesel-cli
      harper
      prek
    ];
  };
}
