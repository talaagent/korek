{
  description = "korek: define types once in KDL, cast them into Rust, Protobuf, Zod and TypeScript";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # rust-toolchain.toml is the one source of the toolchain.
        rustToolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
          extensions = [ "rust-analyzer" ];
        };

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        korek = rustPlatform.buildRustPackage {
          pname = "korek";
          version = "0.1.0";
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./crates
              ./examples
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "-p"
            "korek"
          ];
          # The suite shells out to buf/protoc and bun; run it in the dev shell.
          doCheck = false;
          meta = {
            description = "Define types once in KDL, cast them into Rust, Protobuf, Zod and TypeScript";
            homepage = "https://github.com/talaagent/korek";
            license = pkgs.lib.licenses.asl20;
            mainProgram = "korek";
          };
        };
      in
      {
        packages = {
          inherit korek;
          default = korek;
        };

        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            # The Zod output's tests (bun test) and the TS type check.
            pkgs.bun
            # Lint and compile the generated .proto files in the tests.
            pkgs.buf
            pkgs.protobuf
            pkgs.git
          ];
        };

        formatter = pkgs.nixfmt;
      }
    );
}
