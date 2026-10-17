{
  description = "keep";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/release-26.05";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (pkgsFor system));
      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      rustPlatformFor = pkgs:
        let
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
          pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
    in {
      packages = forAllSystems (pkgs: {
        default = (rustPlatformFor pkgs).buildRustPackage {
          name = "keep";

          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
              ./scripts/take-snapshot.bash
            ];
          };

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            pkg-config
            makeWrapper
          ];

          buildInputs = with pkgs; [
            openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            fuse
            libiconv
          ];

          postInstall = ''
            wrapProgram $out/bin/keep \
              --prefix PATH : ${pkgs.lib.makeBinPath (with pkgs; [ git bash coreutils findutils ])}
          '';

          meta.mainProgram = "keep";
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustup
            pkg-config
            openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            fuse
            libiconv
          ];
        };
      });
    };
}
