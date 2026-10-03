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
        # Keep integration-test edits out of the installed package's inputs.
        # Fixtures stay here because provider code embeds some of them.
        rustFiles = [
          ./Cargo.toml
          ./Cargo.lock
          ./src
          ./tests/fixtures
        ];
        testSrc = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions (rustFiles ++ [ ./tests ]);
        };
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions rustFiles;
        };
        commonArgs = {
          inherit src;
          pname = cargoManifest.package.name;
          version = cargoManifest.package.version;
          CARGO_PROFILE = "dev";
          strictDeps = true;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ];
        };
        # Checks and cargo package verification use dev artifacts; the shipped
        # application has a separate release dependency cache.
        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // { src = testSrc; });
        releaseCargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
          CARGO_PROFILE = "release";
          doCheck = false;
        });
        idxPackage = craneLib.buildPackage (commonArgs // {
          cargoArtifacts = releaseCargoArtifacts;
          CARGO_PROFILE = "release";
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
        installerSrc = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [ ./install.sh ./scripts/install-sh-test.sh ];
        };
        installerCheck = shell: pkgs.runCommand "idx-installer-${shell}" {
          nativeBuildInputs = with pkgs; [
            bash coreutils curl dash findutils gnugrep gnused gawk python3
          ];
        } ''
          cp -r ${installerSrc}/. .
          chmod -R u+w scripts
          patchShebangs scripts
          export HOME="$TMPDIR"
          INSTALL_SH_SHELL=${shell} bash scripts/install-sh-test.sh
          touch "$out"
        '';
        publisherTestSrc = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./scripts/publish-ownership-snapshot-test.sh
            ./scripts/publish-ownership-snapshot.sh
            ./scripts/build-ownership-snapshot.sh
          ];
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
          ownership-publisher = pkgs.runCommand "idx-ownership-publisher-smoke" { } ''
            ${ownershipPublisher}/bin/idx-ownership-publish --help
            touch "$out"
          '';
          fmt = craneLib.cargoFmt { src = testSrc; };
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            src = testSrc;
            doInstallCargoArtifacts = false;
            installPhaseCommand = "touch $out";
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });
          test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
            src = testSrc;
            doInstallCargoArtifacts = false;
            installPhaseCommand = "touch $out";
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
                ./tests
                ./LICENSE
                ./README.md
              ]);
            };
            buildPhaseCargoCommand = "cargo package --locked --offline --allow-dirty";
            doInstallCargoArtifacts = false;
            installPhaseCommand = "touch $out";
          });
          smoke-mock = pkgs.runCommand "idx-smoke-mock"
            { nativeBuildInputs = with pkgs; [ bash coreutils diffutils findutils gnugrep gnused gawk jq ]; }
            ''
              mkdir -p scripts
              cp ${./scripts/live-smoke.sh} scripts/live-smoke.sh
              chmod u+w scripts/live-smoke.sh
              patchShebangs scripts/live-smoke.sh
              # The mock provider reads fixtures relative to the working dir.
              mkdir -p tests
              cp -r ${./tests/fixtures} tests/fixtures
              export HOME="$TMPDIR"
              ${idxPackage}/bin/idx version
              bash scripts/live-smoke.sh --bin ${idxPackage}/bin/idx --no-build --mode mock
              touch "$out"
            '';
          install-script-sh = installerCheck "sh";
          install-script-dash = installerCheck "dash";
          shellcheck = pkgs.runCommand "idx-shellcheck" {
            nativeBuildInputs = [ pkgs.shellcheck ];
          } ''
            shellcheck -s sh ${installerSrc}/install.sh
            shellcheck ${installerSrc}/scripts/install-sh-test.sh
            shellcheck ${./scripts/live-smoke.sh}
            touch "$out"
          '';
          publisher-test = pkgs.runCommand "idx-publisher-test" {
            nativeBuildInputs = with pkgs; [
              bash coreutils findutils gawk gnugrep gnused jq sqlite
            ];
          } ''
            cp -r ${publisherTestSrc}/. .
            chmod -R u+w scripts
            patchShebangs scripts
            export HOME="$TMPDIR"
            bash scripts/publish-ownership-snapshot-test.sh
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
            jq
            diffutils
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
