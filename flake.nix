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
        version = cargoManifest.workspace.package.version;
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
          ./crates/idx-core/Cargo.toml
          ./crates/idx-core/src
          ./crates/idx-core/tests/fixtures
          ./crates/idx-cli/Cargo.toml
          ./crates/idx-cli/src
          ./crates/idx-mcp/Cargo.toml
          ./crates/idx-mcp/src
        ];
        testSrc = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions (rustFiles ++ [ ./crates/idx-cli/tests ./crates/idx-mcp/tests ]);
        };
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions rustFiles;
        };
        commonArgs = {
          inherit src;
          pname = "idx-cli";
          inherit version;
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
          cargoExtraArgs = "--locked -p idx-cli";
          doCheck = false;
          nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram "$out/bin/idx" \
              --prefix PATH : "${lib.makeBinPath runtimeDeps}"
          '';
        });
        mcpPackage = craneLib.buildPackage (commonArgs // {
          pname = "idx-mcp";
          cargoArtifacts = releaseCargoArtifacts;
          CARGO_PROFILE = "release";
          cargoExtraArgs = "--locked -p idx-mcp";
          doCheck = false;
          nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram "$out/bin/idx-mcp" \
              --prefix PATH : "${lib.makeBinPath runtimeDeps}"
          '';
          meta.mainProgram = "idx-mcp";
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
          inherit version;
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
        # `nix develop .#agents-fhs` (Linux): the agents shell, with every agent
        # run inside an FHS sandbox. Hermes runs a bare `npx` with its own
        # generic-Linux Node, which NixOS cannot load, so testing the
        # `npx -y idx-mcp` setup users have needs this on NixOS.
        # `agent-fhs <cmd>` runs any command (e.g. an interactive `hermes`) inside it.
        agentsFhsShell =
          let
            agentFhs = pkgs.buildFHSEnv {
              name = "agent-fhs";
              targetPkgs = p: [ p.stdenv.cc.cc.lib p.zlib p.openssl ];
              runScript = pkgs.writeShellScript "agent-fhs-run" ''exec "''${@:-bash}"'';
            };
          in
          self.devShells.${system}.agents.overrideAttrs (old: {
            shellHook = (old.shellHook or "") + ''
              export PATH="${agentFhs}/bin:$PATH"
              export IDX_AGENT_EXEC_PREFIX="''${IDX_AGENT_EXEC_PREFIX:+$IDX_AGENT_EXEC_PREFIX }agent-fhs"
            '';
          });
      in
      lib.recursiveUpdate {
        packages.default = idxPackage;
        packages.idx-mcp = mcpPackage;
        packages.ownership-publisher = ownershipPublisher;
        apps.default = {
          type = "app";
          program = "${idxPackage}/bin/idx";
          meta.description = "idx CLI";
        };
        checks = {
          default = idxPackage;
          idx-mcp = mcpPackage;
          ownership-publisher = pkgs.runCommand "idx-ownership-publisher-smoke" { } ''
            ${ownershipPublisher}/bin/idx-ownership-publish --help
            touch "$out"
          '';
          fmt = craneLib.cargoFmt { src = testSrc; pname = "idx-cli"; inherit version; };
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
          # `cargo package` verification: each published crate (per the
          # `include` lists in crates/*/Cargo.toml) must build on its own.
          package = craneLib.mkCargoDerivation (commonArgs // {
            inherit cargoArtifacts;
            pnameSuffix = "-package";
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions (rustFiles ++ [
                ./crates/idx-cli/tests
                ./crates/idx-core/LICENSE
                ./crates/idx-cli/LICENSE
                ./crates/idx-mcp/tests
                ./crates/idx-mcp/LICENSE
                ./LICENSE
                ./README.md
              ]);
            };
            buildPhaseCargoCommand = ''
              # idx-core: full `cargo package` verification.
              cargo package -p idx-core --registry crates-io --locked --offline --allow-dirty
              # idx-cli depends on idx-core, which is not on crates.io yet, so
              # cargo cannot resolve it from the vendored registry. Package
              # idx-cli without lockfile or verification, then build the
              # extracted tarball against the packaged idx-core. Real releases
              # publish idx-core first and package idx-cli normally.
              cargo package -p idx-cli --registry crates-io --no-verify --exclude-lockfile --offline --allow-dirty
              verify="$TMPDIR/package-verify"
              mkdir -p "$verify"
              tar -xzf target/package/idx-core-${version}.crate -C "$verify"
              tar -xzf target/package/idx-cli-${version}.crate -C "$verify"
              cp Cargo.lock "$verify/idx-cli-${version}/"
              (cd "$verify/idx-cli-${version}" && cargo build --offline \
                --config 'patch.crates-io.idx-core.path="../idx-core-${version}"' \
                --target-dir "$verify/target")
              # idx-mcp: same as idx-cli.
              cargo package -p idx-mcp --registry crates-io --no-verify --exclude-lockfile --offline --allow-dirty
              tar -xzf target/package/idx-mcp-${version}.crate -C "$verify"
              cp Cargo.lock "$verify/idx-mcp-${version}/"
              (cd "$verify/idx-mcp-${version}" && cargo build --offline \
                --config 'patch.crates-io.idx-core.path="../idx-core-${version}"' \
                --target-dir "$verify/target")
            '';
            doInstallCargoArtifacts = false;
            installPhaseCommand = "touch $out";
          });
          # Sanity check of the packaged (wrapped) binary; CLI behavior is
          # covered by tests/cli.rs.
          packaged-cli = pkgs.runCommand "idx-packaged-cli"
            { nativeBuildInputs = with pkgs; [ bash jq ]; }
            ''
              export HOME="$TMPDIR" XDG_CACHE_HOME="$TMPDIR/cache" XDG_CONFIG_HOME="$TMPDIR/config"
              ${idxPackage}/bin/idx -o json version | jq -e '.version | type == "string"'
              ${idxPackage}/bin/idx completions bash | bash -n
              # Release builds have no mock provider; an offline cache miss
              # exercises the JSON error contract and exit-code classes.
              set +e
              ${idxPackage}/bin/idx --offline -o json stocks quote BBCA 2>err.json
              status=$?
              set -e
              test "$status" -eq 3
              jq -e '.code == "CACHEMISS"' err.json
              touch "$out"
            '';
          # The packaged server answers initialize and lists its tools over
          # stdio; tool behavior is covered by crates/idx-mcp/tests.
          packaged-mcp = pkgs.runCommand "idx-packaged-mcp"
            { nativeBuildInputs = with pkgs; [ coreutils jq ]; }
            ''
              export HOME="$TMPDIR" XDG_CACHE_HOME="$TMPDIR/cache" XDG_CONFIG_HOME="$TMPDIR/config" XDG_DATA_HOME="$TMPDIR/data"
              {
                echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"check","version":"0"}}}'
                echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
                echo '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
                sleep 2
              } | timeout 30 ${mcpPackage}/bin/idx-mcp > out.jsonl
              jq -se 'map(select(.id == 2))[0].result.tools | length == 17' out.jsonl
              touch "$out"
            '';
          # The website's generated pages match the CLI help and the MCP
          # tools golden; regenerate with web/scripts/gen-*.sh.
          web-generated = pkgs.runCommand "idx-web-generated"
            { nativeBuildInputs = with pkgs; [ bash coreutils diffutils gawk gnused jq ]; }
            ''
              export HOME="$TMPDIR" XDG_CACHE_HOME="$TMPDIR/cache" XDG_CONFIG_HOME="$TMPDIR/config"
              bash ${./web/scripts/gen-cli-reference.sh} ${idxPackage}/bin/idx > reference.md
              diff -u ${./web/src/content/docs/cli/reference.md} reference.md
              bash ${./web/scripts/gen-mcp-tools.sh} ${./crates/idx-mcp/tests/golden/tools.json} > tools.md
              diff -u ${./web/src/content/docs/mcp/tools.md} tools.md
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
            shellcheck ${./tests/agents/agent-smoke.sh}
            shellcheck ${./web/scripts/gen-cli-reference.sh} ${./web/scripts/gen-mcp-tools.sh}
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

        # `nix develop .#agents`: the default shell plus coding-agent CLIs for
        # tests/agents/agent-smoke.sh. They are appended to PATH, so agents
        # already installed (and logged in) on the host take precedence.
        # pi and Hermes Agent are not in the pinned nixpkgs; use host installs.
        devShells.agents =
          let
            agentPkgs = import nixpkgs {
              inherit system;
              config.allowUnfreePredicate = pkg: lib.getName pkg == "claude-code";
            };
          in
          self.devShells.${system}.default.overrideAttrs (old: {
            shellHook = (old.shellHook or "") + ''
              export PATH="$PATH:${lib.makeBinPath (with agentPkgs; [ claude-code codex opencode ])}"
            '';
          });

        # `nix develop .#web`: bun for the website in web/.
        devShells.web = pkgs.mkShell {
          packages = with pkgs; [ bun nodejs_22 jq ];
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
      } (lib.optionalAttrs pkgs.stdenv.isLinux { devShells.agents-fhs = agentsFhsShell; })
    );
}
