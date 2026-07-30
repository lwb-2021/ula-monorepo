{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      utils,
      naersk,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        naersk-lib = pkgs.callPackage naersk { };

        nativeBuildInputs = with pkgs; [ pkg-config ];
        buildInputs = with pkgs; [
          openssl
          curl
        ];
      in
      {
        defaultPackage = naersk-lib.buildPackage {
          src = ./.;
          inherit nativeBuildInputs buildInputs;
        };
        devShell =
          with pkgs;
          mkShell {
            nativeBuildInputs = [
              rustfmt
              pre-commit
              rustPackages.clippy
              rust-analyzer
            ]
            ++ nativeBuildInputs;

            buildInputs = [
              cargo
              rustc

              cargo-bloat
            ]
            ++ buildInputs;
            RUST_SRC_PATH = rustPlatform.rustLibSrc;
            RUST_BACKTRACE = 1;
          };
      }
    );
}
