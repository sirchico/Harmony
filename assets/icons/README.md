# Interface icons

[Phosphor Icons](https://phosphoricons.com) by Helena Zhang and Tobias Fried, Copyright (c)
2023 Phosphor Icons, licensed under the [MIT License](LICENSE). Source: npm package
`@phosphor-icons/core` version 2.1.1 (repository https://github.com/phosphor-icons/core),
fetched through the jsDelivr npm mirror on September 10, 2026. The unmodified license is
`LICENSE` and is staged in both packages as `licenses/Phosphor-Icons-MIT.txt`.

One hundred and eight unmodified Phosphor `assets/fill/*.svg` and `assets/bold/*.svg` files are scaled
to 56×56 pixels, filled white, and rasterized by `resvg` 0.45.1 into one transparent PNG atlas
with 64×64 cells (8 columns, 16 rows). `headphones-slash` is derived from `headphones-fill.svg`
by masking a diagonal knockout and adding a 16-unit round-capped stroke, matching the style of
Phosphor's own `*-slash` icons. The application tints glyphs at draw time; no icon font,
JavaScript or per-icon file is bundled.

Ten brand marks Phosphor does not ship (PlayStation, Battle.net, Epic Games, League of Legends,
Riot Games, Bungie, Roblox, Crunchyroll, eBay, Bluesky) come from
[Simple Icons](https://simpleicons.org) npm package `simple-icons` version 16.30.0
(repository https://github.com/simple-icons/simple-icons), released under
[CC0 1.0](LICENSE-SIMPLE-ICONS) and fetched through the same mirror on September 11, 2026.
Their 24-unit glyphs fill the whole view box, so they are drawn at 80 % of the cell glyph size
to match Phosphor's visual weight. Brand marks remain trademarks of their owners; Simple Icons'
legal disclaimer applies. The license file is staged in both packages as
`licenses/Simple-Icons-CC0.txt`.

One repository-drawn glyph, `thread.svg` (four slanted round-capped bars on the same 256-unit
grid), marks threads; it is rasterized with the Phosphor set and carries no upstream license.

`monitor-x.svg` marks stopping this device's screen share. It is Phosphor `monitor-fill.svg`
(MIT, same pinned 2.1.1 package, fetched November 10, 2026) with a 16-unit round-capped X masked
out of the screen where `monitor-arrow-up` draws its arrow, so it pairs with the share glyph.

- `atlas.png`: 512×1024 RGBA, 139,260 bytes as written by `resvg` (not yet passed through
  `oxipng`; the last regeneration ran without it). SHA-256
  `2e55b5806ec65425eacae776e7dc494bc7f5e41ab670dc807fde50825c2760dd`.
- `index.tsv`: icon name, tab, zero-based cell; SHA-256
  `e4031b3a7d37718639a7d563149e3f68fd4da508087d234dbca14c4b0002f7c8`.

Every upstream SVG's SHA-256 is pinned in `tools/generate-icons.py`, which refuses to build
from mismatching files. Regenerate from the repository root:

```sh
cargo install resvg --version 0.45.1 --root /tmp/resvg-tool
python3 tools/generate-icons.py --resvg /tmp/resvg-tool/bin/resvg
```

PNG compression bytes can vary with the resvg/png versions; decoded pixels and cell indices
are deterministic. `cargo test -p ui icons` checks that every `Icon` variant maps to a
distinct, non-blank cell and that the atlas stays below 256 KiB.

The folder and open-folder glyphs are unmodified Phosphor `folder-fill.svg` and
`folder-open-fill.svg`, fetched from the same pinned 2.1.1 package on September 11, 2026.

The media-viewer caret and download glyphs are unmodified Phosphor `caret-left-bold.svg` and
`download-simple-bold.svg`, fetched from the same pinned 2.1.1 package on September 11, 2026.

The screen-share fullscreen glyph is unmodified Phosphor `corners-out-bold.svg`, fetched from the
same pinned 2.1.1 package on September 27, 2026.

The text context-menu glyphs are unmodified Phosphor `scissors-bold.svg`, `clipboard-text-bold.svg`
and `selection-all-bold.svg`, fetched from the same pinned 2.1.1 package on November 6, 2026.

The menu-row glyphs are unmodified Phosphor `at-bold.svg`, `bell-slash-fill.svg`,
`prohibit-bold.svg`, `note-pencil-fill.svg`, `identification-card-fill.svg`,
`envelope-simple-fill.svg`, `sign-out-bold.svg`, `user-minus-fill.svg`, `tag-fill.svg`,
`eye-fill.svg`, `gavel-fill.svg`, `clock-fill.svg`, `speaker-slash-fill.svg` and
`palette-fill.svg`, fetched from the same pinned 2.1.1 package on November 14, 2026.

The `serein-mark` cell is our own artwork, not an upstream icon: it is the solid silhouette of
the Serein chat-wave from `assets/brand/serein-mark.svg`, trimmed to its bounding box so it
matches Phosphor's glyph weight in the cell. It replaced the Discord brand mark that previously
occupied cell 58; no third-party application logo is bundled any more. `tools/generate-icons.py`
pins its SHA-256 like every upstream source and rasterizes it from the repository rather than a
package mirror.
