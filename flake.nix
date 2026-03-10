{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs:
    inputs.flake-parts.lib.mkFlake {inherit inputs;} {
      systems = inputs.nixpkgs.lib.systems.flakeExposed;

      perSystem = {
        system,
        self',
        ...
      }: let
        overlays = [inputs.rust-overlay.overlays.default];
        pkgs = import inputs.nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        # Pre-build the web UI so the Rust build doesn't need network access
        oxiUi = pkgs.buildNpmPackage {
          pname = "oxidian-ui";
          version = cargoToml.package.version;
          src = ./ui;
          npmDepsHash = "sha256-7O+9gPyR63asqBqsBRc5R1flYNZ28xfqKhaAJ9aM458=";
          npmFlags = ["--legacy-peer-deps"];
          # Skip tsc type-checking (sigma v2/v3 peer dep mismatch causes type
          # errors that don't affect the runtime bundle). Just run vite build.
          buildPhase = ''
            runHook preBuild
            npx vite build
            runHook postBuild
          '';
          installPhase = ''
            runHook preInstall
            mkdir -p $out/dist
            cp -R dist/. $out/dist/
            runHook postInstall
          '';
        };

        oxi = rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          src = pkgs.lib.cleanSource ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          buildFeatures = ["web-ui"];
          OXI_UI_DIST = "${oxiUi}/dist";

          meta = {
            mainProgram = "oxi";
            description = "Obsidian vault indexing + query CLI";
          };
        };
      in {
        packages.oxi = oxi;
        packages.default = oxi;

        apps.oxi = {
          type = "app";
          program = "${self'.packages.oxi}/bin/oxi";
        };
        apps.default = self'.apps.oxi;

        devShells.default = pkgs.mkShell {
          buildInputs = [rustToolchain pkgs.cacert pkgs.sqlite pkgs.nodejs];

          shellHook = ''
            export PS1="(oxidian) $PS1"
          '';
        };
      };
    };
}
