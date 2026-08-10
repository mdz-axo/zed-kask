{
  pkgs,
  system,
  lib,
  stdenv,

  cargo-about,
  crane,
  rustPlatform,
  rustToolchain,

  fetchFromGitHub,
  makeFontsConf,
  makeWrapper,

  alsa-lib,
  cmake,
  curl,
  fontconfig,
  freetype,
  glib,
  libdrm,
  libgbm,
  libgit2,
  libglvnd,
  libva,
  libxcomposite,
  libxdamage,
  libxext,
  libxfixes,
  libxkbcommon,
  libxrandr,
  libx11,
  libxcb,
  nodejs_22,
  openssl,
  perl,
  pkg-config,
  protobuf,
  sqlite,
  vulkan-loader,
  wayland,
  xorg,
  zlib,
  zstd,

  withGLES ? false,
  profile ? "release",
  commitSha ? null,
}:
assert withGLES -> stdenv.hostPlatform.isLinux;
let
  mkIncludeFilter =
    root': path: type:
    let
      # note: under lazy-trees this introduces an extra copy
      root = toString root' + "/";
      relPath = lib.removePrefix root path;
      topLevelIncludes = [
        "crates"
        "assets"
        "extensions"
        "script"
        "tooling"
        "Cargo.toml"
        ".config" # nextest?
        ".cargo"
      ];
      firstComp = builtins.head (lib.path.subpath.components relPath);
    in
    builtins.elem firstComp topLevelIncludes;

  craneLib = crane.overrideToolchain rustToolchain;
  gpu-lib = if withGLES then libglvnd else vulkan-loader;
  commonArgs =
    let
      zedCargoLock = builtins.fromTOML (builtins.readFile ../crates/zed/Cargo.toml);
      stdenv' = stdenv;
    in
    rec {
      pname = "zed-editor";
      version =
        zedCargoLock.package.version
        + "-nightly"
        + lib.optionalString (commitSha != null) "+${builtins.substring 0 7 commitSha}";
      src = builtins.path {
        path = ../.;
        filter = mkIncludeFilter ../.;
        name = "source";
      };

      cargoLock = ../Cargo.lock;

      nativeBuildInputs = [
        cmake
        curl
        perl
        pkg-config
        protobuf
        # Pin cargo-about to 0.8.2. Newer versions don't work with the current license identifiers
        # See https://github.com/zed-industries/zed/pull/44012
        (cargo-about.overrideAttrs (
          new: old: rec {
            version = "0.8.2";

            src = fetchFromGitHub {
              owner = "EmbarkStudios";
              repo = "cargo-about";
              tag = version;
              sha256 = "sha256-cNKZpDlfqEXeOE5lmu79AcKOawkPpk4PQCsBzNtIEbs=";
            };

            cargoHash = "sha256-NnocSs6UkuF/mCM3lIdFk+r51Iz2bHuYzMT/gEbT/nk=";

            # NOTE: can drop once upstream uses `finalAttrs` here:
            # https://github.com/NixOS/nixpkgs/blob/10214747f5e6e7cb5b9bdf9e018a3c7b3032f5af/pkgs/build-support/rust/build-rust-package/default.nix#L104
            #
            # See (for context): https://github.com/NixOS/nixpkgs/pull/382550
            cargoDeps = rustPlatform.fetchCargoVendor {
              inherit (new) src;
              hash = new.cargoHash;
              patches = new.cargoPatches or [ ];
              name = new.cargoDepsName or new.finalPackage.name;
            };
          }
        ))
        rustPlatform.bindgenHook
        makeWrapper
      ];

      buildInputs = [
        curl
        fontconfig
        freetype
        # TODO: need staticlib of this for linking the musl remote server.
        # should make it a separate derivation/flake output
        # see https://crane.dev/examples/cross-musl.html
        libgit2
        openssl
        sqlite
        zlib
        zstd
        alsa-lib
        glib
        libva
        libxkbcommon
        wayland
        gpu-lib
        libglvnd
        libx11
        libxcb
        libdrm
        libgbm
        libva
        libxcomposite
        libxdamage
        libxext
        libxfixes
        libxrandr
      ];

      cargoExtraArgs = "-p zed -p cli --locked --features=gpui_platform/runtime_shaders";

      stdenv = pkgs:
        let
          base = pkgs.llvmPackages.stdenv;
          addBinTools = old: {
            cc = old.cc.override {
              inherit (pkgs.llvmPackages) bintools;
            };
          };
        in
        lib.pipe base [
          (stdenv: stdenv.override addBinTools)
          pkgs.stdenvAdapters.useMoldLinker
        ];

      env = {
        ZSTD_SYS_USE_PKG_CONFIG = true;
        FONTCONFIG_FILE = makeFontsConf {
          fontDirectories = [
            ../assets/fonts/lilex
            ../assets/fonts/ibm-plex-sans
          ];
        };
        ZED_UPDATE_EXPLANATION = "Zed has been installed using Nix. Auto-updates have thus been disabled.";
        RELEASE_VERSION = version;
        ZED_COMMIT_SHA = lib.optionalString (commitSha != null) "${commitSha}";
        LK_CUSTOM_WEBRTC = pkgs.callPackage ./livekit-libwebrtc/package.nix { };
        PROTOC = "${protobuf}/bin/protoc";

        CARGO_PROFILE = profile;
        # need to handle some profiles specially https://github.com/rust-lang/cargo/issues/11053
        TARGET_DIR = "target/" + (if profile == "dev" then "debug" else profile);

        # for some reason these deps being in buildInputs isn't enough, the only thing
        # about them that's special is that they're manually dlopened at runtime
        NIX_LDFLAGS = "-rpath ${
          lib.makeLibraryPath [
            gpu-lib
            wayland
            libva
          ]
        }";

        NIX_OUTPATH_USED_AS_RANDOM_SEED = "norebuilds";
      };

      # prevent nix from removing the "unused" wayland/gpu-lib rpaths
      dontPatchELF = true;

      # TODO: try craneLib.cargoNextest separate output
      # for now we're not worried about running our test suite (or tests for deps) in the nix sandbox
      doCheck = false;

      cargoVendorDir = craneLib.vendorCargoDeps {
        inherit src cargoLock;
        overrideVendorGitCheckout =
          let
            hasWebRtcSys = builtins.any (crate: crate.name == "webrtc-sys");
            # we can't set $RUSTFLAGS because that clobbers the cargo config
            # see https://github.com/rust-lang/cargo/issues/5376#issuecomment-2163350032
            glesConfig = builtins.toFile "config.toml" ''
              [target.'cfg(all())']
              rustflags = ["--cfg", "gles"]
            '';

            # `webrtc-sys` expects a staticlib; nixpkgs' `livekit-webrtc` has been patched to
            # produce a `dylib`... patching `webrtc-sys`'s build script is the easier option
            # TODO: send livekit sdk a PR to make this configurable
            postPatch = ''
              substituteInPlace webrtc-sys/build.rs --replace-fail \
                "cargo:rustc-link-lib=static=webrtc" "cargo:rustc-link-lib=dylib=webrtc"

              substituteInPlace webrtc-sys/build.rs --replace-fail \
                'add_gio_headers(&mut builder);' \
                'for lib_name in ["glib-2.0", "gio-2.0"] {
                    if let Ok(lib) = pkg_config::Config::new().cargo_metadata(false).probe(lib_name) {
                        for path in lib.include_paths {
                            builder.include(&path);
                        }
                    }
                }'
            ''
            + lib.optionalString withGLES ''
              cat ${glesConfig} >> .cargo/config/config.toml
            '';
          in
          crates: drv:
          if hasWebRtcSys crates then
            drv.overrideAttrs (o: {
              postPatch = (o.postPatch or "") + postPatch;
            })
          else
            drv;
      };
    };
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (
  lib.recursiveUpdate commonArgs {
    inherit cargoArtifacts;

    # Expose the crane builder and shared arguments so other derivations (e.g.
    # the docs preprocessor in the devshell) can build sibling workspace crates
    # without duplicating all of the build inputs and environment setup.
    passthru = {
      inherit craneLib commonArgs cargoArtifacts;
    };

    dontUseCmakeConfigure = true;

    # without the env var generate-licenses fails due to crane's fetchCargoVendor, see:
    # https://github.com/zed-industries/zed/issues/19971#issuecomment-2688455390
    # TODO: put this in a separate derivation that depends on src to avoid running it on every build
    preBuild = ''
      ALLOW_MISSING_LICENSES=yes bash script/generate-licenses
      echo nightly > crates/zed/RELEASE_CHANNEL
    '';

    # zed-kask: installPhase is build-only (consumed by the devshell, not packaged).
    # The installable zed-kask binary is produced by kask/scripts/build/install.sh.
    installPhase = ''
      runHook preInstall

      mkdir -p $out/bin $out/libexec
      cp $TARGET_DIR/zed $out/libexec/zed-editor
      cp $TARGET_DIR/cli $out/bin/zed-kask

      runHook postInstall
    '';

    postFixup = ''
      wrapProgram $out/libexec/zed-editor --suffix PATH : ${lib.makeBinPath [ nodejs_22 ]}
    '';

    meta = {
      description = "zed-kask — fork of the Zed code editor with hKask agent infrastructure";
      homepage = "https://github.com/mdz-axo/zed-kask";
      license = lib.licenses.gpl3Only;
      mainProgram = "zed-kask";
      platforms = lib.platforms.linux;
    };
  }
)
