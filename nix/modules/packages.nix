{ inputs, ... }:
{
  perSystem =
    {
      pkgs,
      lib,
      system,
      ...
    }:
    :
    let
    in
    {
    // lib.optionalAttrs (lib.hasSuffix "linux" system) {
      checks = {
        a11y-test = import ../tests/a11y.nix {
          inherit pkgs inputs;
        };
      }
      // import ../tests/sandboxing { inherit pkgs inputs; };
    };
}
