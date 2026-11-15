# Harmony brand assets

Harmony's application icon is a glowing Ferris (the Rust mascot) on a dark rounded plate. The
in-app mark is a plain white silhouette of Ferris.

- `harmony-1024.png`: 1024x1024 RGBA render of the application icon, upscaled from the 640px
  master supplied for the fork.
- `harmony.svg`, `harmony-flat.svg`: scalable wrappers that embed a 256px render of the same icon.
  They are raster images inside an SVG container, not vector artwork.
- `harmony-mark.svg`: solid white silhouette of Ferris on a 256-unit grid, built from horizontal
  pixel runs of the mascot art. `tools/generate-icons.py` rasterizes it into the shared icon atlas
  as `harmony-mark`, which the interface tints at draw time, and the Linux tray recolours it.
- `harmony-tray.png`: 72x72 black-on-transparent render of the same silhouette, embedded by
  `crates/platform` as the macOS menu bar template image; only its alpha is used.

The packaged platform icons built from the same artwork live in `packaging/` (`windows/Harmony.ico`,
`windows/harmony.png`, `macos/Harmony.icns`, `linux/hicolor/*`). `packaging/macos/Harmony.icon` still
holds the layered Icon Composer source carried over from an earlier version and has not been redrawn.

Ferris was created by Karen Rustad Tölva and is dedicated to the public domain (CC0); see
https://rustacean.net. These assets are not Discord's or any other vendor's branding.

The interface accent (`design::DEFAULT_PRIMARY_RGB`) is Discord blurple, `#5865f2`; white text on
it keeps a 4.5:1 contrast ratio.
