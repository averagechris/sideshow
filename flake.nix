{
  description = "Agentic HTML slide deck compiler and toolkit";

  nixConfig = {
    extra-substituters = ["https://averagechris-dotfiles.cachix.org"];
    extra-trusted-public-keys = ["averagechris-dotfiles.cachix.org-1:VwJkl5dG1+xGDY5x884mH/kVwwpgwBAdBKIF3BZiia4="];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fleet.url = "git+https://git.sr.ht/~averagechris/averagechris.srht.site";
  };

  outputs = {
    self,
    nixpkgs,
    fleet,
  }: let
    systems = [
      "aarch64-darwin"
      "aarch64-linux"
      "x86_64-darwin"
      "x86_64-linux"
    ];

    forAllSystems = nixpkgs.lib.genAttrs systems;
    pkgsFor = system: import nixpkgs {inherit system;};
    cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    package = cargoToml.package;
    fleetApps = system:
      fleet.lib.fleet.presets.rust {
        pkgs = pkgsFor system;
        inherit self;
        srhtPackage = fleet.packages.${system}.srht;
        pname = "sideshow";
        binaries = ["sideshow"];
        subdir = "sideshow";
        srhtRepo = "sideshow";
        versionMode = "package";
        versionFile = "Cargo.toml";
        lockPackages = ["sideshow"];
        releaseValidationApps = [
          "ci-deny"
          "ci-machete"
          "ci-sort"
          "ci-release-contract"
        ];
      };
    mkToolApp = system: name: runtimeInputs: text: let
      pkgs = pkgsFor system;
    in
      pkgs.writeShellApplication {
        inherit name runtimeInputs text;
      };
    ciAudit = system:
      mkToolApp system "ci-audit" [(pkgsFor system).cargo (pkgsFor system).cargo-audit] ''
        cargo audit --deny warnings
      '';
    ciDeny = system:
      mkToolApp system "ci-deny" [(pkgsFor system).cargo (pkgsFor system).cargo-deny] ''
        cargo deny check
      '';
    ciMachete = system:
      mkToolApp system "ci-machete" [(pkgsFor system).cargo (pkgsFor system).cargo-machete] ''
        cargo machete
      '';
    ciSort = system:
      mkToolApp system "ci-sort" [(pkgsFor system).cargo (pkgsFor system).cargo-sort] ''
        cargo sort --workspace --check
      '';
    ciReleaseContract = system:
      mkToolApp system "ci-release-contract" [(pkgsFor system).bash (pkgsFor system).coreutils (pkgsFor system).gnugrep (pkgsFor system).nix] ''
        exec bash scripts/ci-release-contract.sh
      '';
    ciTest = system: let
      pkgs = pkgsFor system;
    in
      mkToolApp system "ci-test" [pkgs.coreutils pkgs.tailwindcss_4] ''
        env \
          -u SIDESHOW_CONFIG \
          -u SIDESHOW_TAILWINDCSS \
          PATH=${pkgs.lib.makeBinPath [pkgs.coreutils pkgs.tailwindcss_4]} \
          ${(fleetApps system).apps.ci-test.program}

        composition_test=""
        for candidate in target/debug/deps/composition-*; do
          if [[ -x "$candidate" && ( -z "$composition_test" || "$candidate" -nt "$composition_test" ) ]]; then
            composition_test="$candidate"
          fi
        done
        if [[ -z "$composition_test" ]]; then
          echo "composition test binary not found" >&2
          exit 1
        fi

        exec env \
          -u SIDESHOW_CONFIG \
          -u SIDESHOW_TAILWINDCSS \
          PATH=${pkgs.lib.makeBinPath [pkgs.coreutils]} \
          "$composition_test"
      '';
    avifEvaluationEncoder = system: enableAvif: let
      pkgs = pkgsFor system;
    in
      pkgs.rustPlatform.buildRustPackage {
        pname = "sideshow-avif-evaluation-encoder${
          if enableAvif
          then ""
          else "-webp-only"
        }";
        version = "1";
        src = ./tools/avif-evaluation/encoder;
        cargoLock.lockFile = ./tools/avif-evaluation/encoder/Cargo.lock;
        buildFeatures = pkgs.lib.optional enableAvif "avif";
      };
    avifEvaluation = system:
      mkToolApp system "avif-evaluation" [
        (avifEvaluationEncoder system true)
        (pkgsFor system).cacert
        (pkgsFor system).ffmpeg
        (pkgsFor system).python3
      ] ''
        exec python3 ${./tools/avif-evaluation/run.py} "$@"
      '';
    nixFormatter = system: let
      pkgs = pkgsFor system;
    in
      pkgs.writeShellApplication {
        name = "alejandra";
        runtimeInputs = [pkgs.alejandra];
        text = ''
          if [[ $# -eq 0 ]]; then
            exec alejandra -q .
          fi

          exec alejandra -q "$@"
        '';
      };
  in {
    packages = forAllSystems (system: let
      pkgs = pkgsFor system;
      lib = pkgs.lib;
      app = pkgs.rustPlatform.buildRustPackage {
        pname = "sideshow";
        version = package.version;
        src = lib.cleanSource ./.;
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [pkgs.makeWrapper];

        postInstall = ''
          wrapProgram $out/bin/sideshow \
            --prefix PATH : ${lib.makeBinPath [pkgs.tailwindcss_4]}
        '';

        meta = {
          description = package.description;
          license = with lib.licenses; [mit asl20];
          mainProgram = "sideshow";
        };
      };
    in {
      default = app;
      sideshow = app;
      ci-audit = ciAudit system;
      ci-deny = ciDeny system;
      ci-machete = ciMachete system;
      ci-sort = ciSort system;
      ci-release-contract = ciReleaseContract system;
      avif-evaluation-encoder = avifEvaluationEncoder system true;
      avif-evaluation-encoder-webp-only = avifEvaluationEncoder system false;
      release-artifact = (fleetApps system).releaseArtifact system;
    });

    apps = forAllSystems (system: {
      default = self.apps.${system}.sideshow;
      sideshow = {
        type = "app";
        program = "${self.packages.${system}.sideshow}/bin/sideshow";
      };
      ci-audit = {
        type = "app";
        program = "${self.packages.${system}.ci-audit}/bin/ci-audit";
      };
      ci-deny = {
        type = "app";
        program = "${self.packages.${system}.ci-deny}/bin/ci-deny";
      };
      ci-machete = {
        type = "app";
        program = "${self.packages.${system}.ci-machete}/bin/ci-machete";
      };
      ci-sort = {
        type = "app";
        program = "${self.packages.${system}.ci-sort}/bin/ci-sort";
      };
      ci-release-contract = {
        type = "app";
        program = "${self.packages.${system}.ci-release-contract}/bin/ci-release-contract";
      };
      avif-evaluation = {
        type = "app";
        program = "${avifEvaluation system}/bin/avif-evaluation";
      };
      ci-test = {
        type = "app";
        program = "${ciTest system}/bin/ci-test";
      };
      inherit ((fleetApps system).apps) prepare-release release-tag release ci-fmt ci-clippy static-checks;
    });

    checks = forAllSystems (system: {
      inherit (self.packages.${system}) sideshow release-artifact;
    });

    devShells = forAllSystems (system: let
      pkgs = pkgsFor system;
    in {
      default = pkgs.mkShell {
        packages = with pkgs; [
          alejandra
          cargo
          cargo-audit
          cargo-deny
          cargo-machete
          cargo-outdated
          cargo-sort
          clippy
          direnv
          ffmpeg
          hut
          jujutsu
          nixd
          rust-analyzer
          rustc
          rustfmt
          sccache
          tailwindcss_4
          vhs
        ];
      };
    });

    formatter = forAllSystems nixFormatter;
  };
}
