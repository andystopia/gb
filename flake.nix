{
  inputs = {
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, fenix, flake-utils, nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system: {
      packages.default =
        let
          toolchain = fenix.packages.${system}.minimal.toolchain;
          pkgs = nixpkgs.legacyPackages.${system};
        in

        (pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
          cargoSha266 = "";
        }).buildRustPackage rec {
          nativeBuildInputs = with pkgs; [  llvmPackages_15.bintools ];
          pname = "gb";
          version = "0.1.0";

          src = pkgs.fetchFromGitHub { 
            owner = "andystopia";
            repo = "gb";
            rev = "e9f6ba61daf08e11f847cd31c61e658b0a395d72";
            hash = "sha256-FJUEantrz4+6cWsSPy42no2ktXzs+yVKxiYt1Fh4rG4=";
            # ref = "main";
            fetchSubmodules = true;
          };

          cargoLock.lockFile = "${src}/Cargo.lock";
          cargoLock.allowBuiltinFetchGit = true;
        };
    });
}
