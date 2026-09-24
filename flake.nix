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
        # Self-contained ownership snapshot publisher for scheduled hosts
        # (e.g. a NixOS systemd timer): no checkout, `nix develop` or cargo
        # build at run time. Wraps the repo scripts with a pinned `idx`.
        publisherTools = with pkgs; [
          bash
          coreutils
          curl
          gawk
          gh
          gnugrep
          gnused
          jq
          sqlite
        ];
        ownershipPublisher = pkgs.stdenvNoCC.mkDerivation {
          pname = "idx-ownership-publisher";
          version = cargoManifest.package.version;
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./scripts/build-ownership-snapshot.sh
              ./scripts/build-latest-ownership-snapshot.sh
              ./scripts/publish-ownership-snapshot.sh
              ./scripts/check-ownership-snapshot-freshness.sh
            ];
          };
          nativeBuildInputs = [ pkgs.makeWrapper ];
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            mkdir -p "$out/libexec/idx-ownership" "$out/bin"
            cp scripts/*.sh "$out/libexec/idx-ownership/"
            chmod +x "$out"/libexec/idx-ownership/*.sh
            patchShebangs "$out/libexec/idx-ownership"

            makeWrapper "$out/libexec/idx-ownership/publish-ownership-snapshot.sh" \
              "$out/bin/idx-ownership-publish" \
              --prefix PATH : "${lib.makeBinPath ([ idxPackage ] ++ publisherTools)}" \
              --add-flags "--idx-bin ${idxPackage}/bin/idx"
            makeWrapper "$out/libexec/idx-ownership/check-ownership-snapshot-freshness.sh" \
              "$out/bin/idx-ownership-freshness" \
              --prefix PATH : "${lib.makeBinPath publisherTools}"
            runHook postInstall
          '';
          meta.mainProgram = "idx-ownership-publish";
        };
      in
      {
        packages.default = idxPackage;
        packages.ownership-publisher = ownershipPublisher;
        apps.default = {
          type = "app";
          program = "${idxPackage}/bin/idx";
        };
        checks.default = idxPackage;
        checks.ownership-publisher = ownershipPublisher;

        devShells.default = pkgs.mkShell {
          inputsFrom = [ idxPackage ];
          packages = with pkgs; [
            rustToolchain
            nodejs_22
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
