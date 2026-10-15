{ pkgs, lib, ... }:

{
  languages.rust = {
    enable = true;
    channel = "nightly";
    version = "latest";
    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "rust-src" ];
  };

  packages = with pkgs;
    [ pkg-config bacon cargo-audit cargo-deny ]
    ++ lib.optionals stdenv.hostPlatform.isLinux [ alsa-lib udev ];

  env.RUST_BACKTRACE = "1";
}
