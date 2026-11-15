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

## Composer and message tools

| Package | Grants | Behavior |
| --- | --- | --- |
| **Offline Composer Tools** | `composer` | Five offline draft tools: clear URL trackers, unindent code blocks, convert units, transform text (`/mock`, `/reverse`, `/morse`, `/wordcount`) and character count. |
| **Text Commands** | `composer`, `storage` | Up to eight literal replacement rules and eight message tags with arguments. Edits are proposals you apply. |
| **ViewRaw** | `selected_message`, `clipboard_write` | A hover-toolbar button showing a message's raw text and data, with copy buttons. |
| **External Search Tools** | `selected_message`, `external_navigation`, `storage` | Translate (hover-toolbar button) or search a message's text in the browser with a chosen translator, language and engine. |

Each plugin README describes its exact behavior. Build them like the others:

```sh
python pack.py plugins/offline-composer-tools/manifest.json target/wasm32-unknown-unknown/release/offline_composer_tools.wasm plugins/packages/offline-composer-tools.harmony-extension
python pack.py plugins/text-commands/manifest.json target/wasm32-unknown-unknown/release/text_commands.wasm plugins/packages/text-commands.harmony-extension
python pack.py plugins/view-raw-message/manifest.json target/wasm32-unknown-unknown/release/view_raw_message.wasm plugins/packages/view-raw-message.harmony-extension
python pack.py plugins/external-search-tools/manifest.json target/wasm32-unknown-unknown/release/external_search_tools.wasm plugins/packages/external-search-tools.harmony-extension
```

## API v2 extensions (Preview)

Written for Harmony from each plugin's described behavior (no third-party code), licensed
MIT OR Apache-2.0, and built only from the public [API v2 SDK](../../docs/extension-sdk-v2.md).
Each is off until enabled in Settings > Extensions and asks for its permissions there.

| Package | Grants | Behavior |
| --- | --- | --- |
| **Base64 Decoder** | `selected_message`, `clipboard_write` | A hover-toolbar button that shows the decoded Base64 text of a message in a window, with a copy button per result; a setting makes it copy directly. |
| **Reverse Image Search** | `media_links`, `external_navigation` | A **Search Image** submenu on images: Google, Yandex, SauceNAO, IQDB, Bing, TinEye, ImgOps, or All. The browser fetches the image; the extension fetches nothing. |
| **Server Info** | `context_menus`, `server_profiles` | **Server Info** in a server's menu: icon, name and description, then owner, creation date, vanity link, locale, verification, boosts, members, roles and banner. |
| **Copy File Contents** | `media_links`, `attachment_contents`, `clipboard_write` | A copy button on text file attachments (up to 64 KiB) that copies the file at once and shows a check mark. |
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
