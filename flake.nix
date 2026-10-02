{
  description = "idx-cli — CLI tool for Indonesian stock market (IDX) analysis";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        lib = pkgs.lib;
        cargoManifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);
        runtimeDeps = with pkgs; [
          curl-impersonate
          mupdf
        ];
        # Only the crate itself (manifests, src/, tests/), so doc or script
        # edits do not invalidate the Rust derivations.
        rustFiles = [
          ./Cargo.toml
          ./Cargo.lock
          ./src
          ./tests
        ];
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions rustFiles;
        };
        commonArgs = {
          inherit src;
          pname = cargoManifest.package.name;
          version = cargoManifest.package.version;
          strictDeps = true;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ];
        };
        # Dependency-only build keyed on Cargo.toml/Cargo.lock: reused from the
        # Nix store (or binary cache) until the dependency graph changes.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        idxPackage = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          doCheck = false;
          nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram "$out/bin/idx" \
              --prefix PATH : "${lib.makeBinPath runtimeDeps}"
          '';
        });
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
        publisherScripts = [
          ./scripts/build-ownership-snapshot.sh
          ./scripts/build-latest-ownership-snapshot.sh
          ./scripts/publish-ownership-snapshot.sh
          ./scripts/check-ownership-snapshot-freshness.sh
        ];
        ownershipPublisher = pkgs.stdenvNoCC.mkDerivation {
          pname = "idx-ownership-publisher";
          version = cargoManifest.package.version;
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions publisherScripts;
          };
          nativeBuildInputs = [ pkgs.makeWrapper ];
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            mkdir -p "$out/libexec/idx-ownership" "$out/bin"
            cp ${lib.concatMapStringsSep " " (script: "scripts/${baseNameOf script}") publisherScripts} \
              "$out/libexec/idx-ownership/"
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
          meta.description = "idx CLI";
        };
        checks = {
          default = idxPackage;
          ownership-publisher = ownershipPublisher;
          fmt = craneLib.cargoFmt { inherit src; };
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });
          test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
            # Ownership discover tests point IDX_CURL_IMPERSONATE_BIN at plain
            # `curl` against a local fixture server.
            nativeCheckInputs = [ pkgs.curl ];
          });
          # `cargo package` verification: the published crate (per the
          # `include` list in Cargo.toml) must build on its own.
          package = craneLib.mkCargoDerivation (commonArgs // {
            inherit cargoArtifacts;
            pnameSuffix = "-package";
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions (rustFiles ++ [
                ./LICENSE
                ./README.md
              ]);
            };
            buildPhaseCargoCommand = "cargo package --locked --offline --allow-dirty";
            doInstallCargoArtifacts = false;
            installPhaseCommand = "touch $out";
          });
          smoke-mock = pkgs.runCommand "idx-smoke-mock"
            { nativeBuildInputs = with pkgs; [ bash coreutils findutils gnugrep gnused gawk ]; }
            ''
              cp -r ${./scripts} scripts
              chmod -R u+w scripts
              patchShebangs scripts
              # The mock provider reads fixtures relative to the working dir.
              mkdir -p tests
              cp -r ${./tests/fixtures} tests/fixtures
              export HOME="$TMPDIR"
              ${idxPackage}/bin/idx version
              bash scripts/live-smoke.sh --bin ${idxPackage}/bin/idx --no-build --mode mock
              touch "$out"
            '';
        };

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
