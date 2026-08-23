{
  toolchainFor = p:
    with p.fenix;
      combine [
        stable.cargo
        stable.rustc
      ];

  devToolchainFor = p:
    with p.fenix;
      combine [
        stable.cargo
        stable.clippy
        stable.rustc
        stable.rustfmt
        stable.rust-analyzer
        stable.rust-src
        stable.llvm-tools
      ];
}
