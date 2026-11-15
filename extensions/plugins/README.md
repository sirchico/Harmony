# Catalog plugins

The published plugins, built against the one [SDK](../sdk) that
the [authoring guide](../../examples/extensions/README.md) documents. Any language
producing compatible WebAssembly can use this ABI; Rust authors can call
`harmony_extension_sdk::export!(handler)`.

Build and package from `extensions/`, the workspace root (Python 3 is used only by the author):

```powershell
rustup target add wasm32-unknown-unknown
cargo build --locked --release --target wasm32-unknown-unknown
python pack.py plugins/message-delete-protector/manifest.json target/wasm32-unknown-unknown/release/message_delete_protector.wasm plugins/packages/message-delete-protector.harmony-extension
```

A rebuilt module is not a release by itself: committed packages are the reviewed bytes
clients install. Replace one only with a reviewed rebuild and a bumped `manifest.version`.

Import the package in Settings > Extensions, review the capabilities, and enable it.
Message delete protector is an example activation plugin. Its `activation` action returns
`preserve_deleted_messages: true` after the user grants `deleted_messages`. The host
keeps already-loaded messages in bounded session memory and displays deleted text in red
by default. Hover and a local context menu can toggle that highlight or remove the
retained row. They never call Discord. The host never sends message bodies to the plugin,
saves deleted bodies to disk, restores messages deleted before loading, or gives deleted
messages live service actions.
Disabling, logout, permission revocation and timeline eviction release retained content.
Emoji and sticker image fallback is now built into the client; the former Emoji &
Sticker Images plugin is retired. Native eligibility uses the account's Nitro
entitlement. Unavailable native selections enter the composer as named artwork
links; explicit Send uses Markdown with text, or attachments for artwork alone.
Declarative themes are under `../themes/`.
The author packages compiled bytes; Harmony never runs a repository's build scripts.

## Equicord-derived safe tools

These independent Rust adaptations use the same real Wasm sandbox and versioned SDK as the
original plugins; they do not evaluate Equicord JavaScript or add automatic send hooks.

| Package / upstream workflows | Grants | Behavior |
| --- | --- | --- |
| **Offline Composer Tools** — ClearURLs, Unindent, UnitConverter, MoreCommands, CharacterCounter | `composer` | Five invoked draft/count tools. URL cleanup uses a fixed offline tracker list; units and text transformations are deterministic; nothing is fetched or sent. |
| **Text Commands** — TextReplace, CustomCommands | `composer`, `storage` | Up to eight literal replacement rules and eight local message tags. Configuration stays bounded and corrupt storage is preserved. Composer edits require Apply. |
| **ViewRaw — safe message view** — ViewRaw | `selected_message` | Displays the selected raw text and public message IDs only; never exposes raw private host objects. |
| **External Search Tools** — Translate, ReplaceGoogleSearch | `selected_message`, `external_navigation`, `storage` | Configures a browser translation/search provider and target language; proposes a complete public HTTPS URL for Apply. No inline network translation, arbitrary request API, or silent navigation. |

Original Vencord notices and GPL-3.0-or-later terms are retained in the sources and
`../licenses/Equicord-GPL-3.0.txt`. See each plugin README for exact behavior and source commit.
Packaging is the same as above, plus `--license` so each package carries the GPL text that
`test_catalog.py` requires:

```sh
python pack.py plugins/offline-composer-tools/manifest.json target/wasm32-unknown-unknown/release/offline_composer_tools.wasm plugins/packages/offline-composer-tools.harmony-extension --license licenses/Equicord-GPL-3.0.txt
python pack.py plugins/text-commands/manifest.json target/wasm32-unknown-unknown/release/text_commands.wasm plugins/packages/text-commands.harmony-extension --license licenses/Equicord-GPL-3.0.txt
python pack.py plugins/view-raw-message/manifest.json target/wasm32-unknown-unknown/release/view_raw_message.wasm plugins/packages/view-raw-message.harmony-extension --license licenses/Equicord-GPL-3.0.txt
python pack.py plugins/external-search-tools/manifest.json target/wasm32-unknown-unknown/release/external_search_tools.wasm plugins/packages/external-search-tools.harmony-extension --license licenses/Equicord-GPL-3.0.txt
```

## API v2 extensions (Preview)

Written for Harmony from each plugin's described behavior (no third-party code), licensed
MIT OR Apache-2.0, and built only from the public [API v2 SDK](../../docs/extension-sdk-v2.md).
Each is off until enabled in Settings > Extensions and asks for its permissions there.

| Package | Grants | Behavior |
| --- | --- | --- |
| **Base64 Decoder** | `selected_message`, `composer`, `clipboard_write` | Decodes Base64 runs in a chosen message (standard and URL-safe, configurable minimum length) and offers to copy the first result; encodes the draft on request. |
| **Reverse Image Search** | `selected_message`, `media_links`, `external_navigation` | Opens an image from a chosen message in Google Lens, Bing, Yandex, TinEye, SauceNAO or IQDB (default engine is a setting); the browser fetches the image, the extension fetches nothing. |
| **Server Info** | `context_menus`, `server_profiles`, `clipboard_write` | Shows a joined server's public details from its menu: owner, creation date, members, boosts, verification and filters, features. Can offer to copy the server ID. |
| **Copy File Contents** | `selected_message`, `media_links`, `attachment_contents`, `clipboard_write` | Reads up to 64 KiB of a text attachment on a chosen message, shows a preview and offers to copy it; other text files on the message get their own buttons. |
| **User Tags** | `context_menus`, `storage`, `ui_contributions` | Put your own colored label on people from their menu; it appears after their name in chat and the member list. A composer button lists and removes tags. Tags stay in local storage. |

Build and package them like any plugin:

```sh
cargo build --locked --release --target wasm32-unknown-unknown -p base-decoder -p reverse-image-search -p server-info -p copy-file-contents -p user-tags
python pack.py plugins/base-decoder/manifest.json target/wasm32-unknown-unknown/release/base_decoder.wasm plugins/packages/base-decoder.harmony-extension
python pack.py plugins/reverse-image-search/manifest.json target/wasm32-unknown-unknown/release/reverse_image_search.wasm plugins/packages/reverse-image-search.harmony-extension
python pack.py plugins/server-info/manifest.json target/wasm32-unknown-unknown/release/server_info.wasm plugins/packages/server-info.harmony-extension
python pack.py plugins/copy-file-contents/manifest.json target/wasm32-unknown-unknown/release/copy_file_contents.wasm plugins/packages/copy-file-contents.harmony-extension
python pack.py plugins/user-tags/manifest.json target/wasm32-unknown-unknown/release/user_tags.wasm plugins/packages/user-tags.harmony-extension
```

## ABI and limits

See [ABI version 1](../../examples/extensions/README.md#abi-version-1) in the SDK guide
for exports, input/output fields, panels and sandbox limits.

## Publishing

Add or update a package through a pull request; see the [extensions README](../README.md).
Maintainers must review the source and built artifact together before listing a version.
All versions and updates require explicit user consent.

## Custom Rich Presence (preview)

See [Custom RPC](custom-rpc/README.md) for setup and builds. Requires the preview
rich-presence SDK from Harmony PR #465. Install through the shared catalog or import
the package locally. Older clients without this capability cannot install it.

## API Proxy (preview)

See [API Proxy](api-proxy/README.md) for the native REST-only proxy editor.
Requires the API proxy SDK capability; Gateway, CDN/media and calls stay direct.
