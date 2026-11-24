{
  description = "Batman: Return of the Joker — Rust/Bevy port dev environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      fenixPkgs = fenix.packages.${system};
      rustToolchain = fenixPkgs.combine [
        fenixPkgs.stable.toolchain
        fenixPkgs.targets.x86_64-pc-windows-gnu.stable.rust-std
      ];
      runtimeLibs = with pkgs; [
        alsa-lib
        libGL
        libx11
        libxcursor
        libxi
        libxkbcommon
        libxrandr
        mesa
        udev
        vulkan-loader
        wayland
      ];
      mingw = pkgs.pkgsCross.mingw-msvcrt-x86_64;
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          cargo-deny
          cc65
          mingw.stdenv.cc
          pkg-config
          python3
          rustToolchain
          xvfb-run
        ];

        buildInputs = runtimeLibs;

        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
        CA65 = "${pkgs.cc65}/bin/ca65";
        LD65 = "${pkgs.cc65}/bin/ld65";
        CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER =
          "${mingw.stdenv.cc}/bin/x86_64-w64-mingw32-gcc";
        CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS =
          "-L native=${mingw.windows.pthreads}/lib";
      };
    };
}
