{
  description = "idx-cli — CLI tool for Indonesian stock market (IDX) analysis";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        lib = pkgs.lib;
        cargoManifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        runtimeDeps = with pkgs; [
          curl-impersonate
          mupdf
        ];
        idxPackage = rustPlatform.buildRustPackage {
          pname = cargoManifest.package.name;
          version = cargoManifest.package.version;
          src = lib.cleanSource ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          doCheck = false;
          nativeBuildInputs = with pkgs; [
            makeWrapper
            pkg-config
          ];
          buildInputs = with pkgs; [
            openssl
          ];
          postInstall = ''
            wrapProgram "$out/bin/idx" \
              --prefix PATH : "${lib.makeBinPath runtimeDeps}"
          '';
        };
      in
      {
        packages.default = idxPackage;
        apps.default = {
          type = "app";
          program = "${idxPackage}/bin/idx";
        };
        checks.default = idxPackage;

        devShells.default = pkgs.mkShell {
          inputsFrom = [ idxPackage ];
          packages = with pkgs; [
            rustToolchain
            cargo-watch
            cargo-nextest
            prek
          ] ++ runtimeDeps;

          shellHook = ''
            export PATH="$PWD/target/debug:$PATH"
          '';

          env = {
            RUST_BACKTRACE = "1";
          };
        };
      }
    );
}
