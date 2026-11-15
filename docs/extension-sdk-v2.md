# Extension API v2 (Preview)

> **Preview — not yet released.** API v2 adds settings, secrets, key-value storage, host
> requests, dependencies and resource quotas to the existing SDK. Version 1 packages keep
> working unchanged. Names may still change before release.

API v2 is an extension of the [existing SDK](extension-sdk-overview.md), not a second one. The
same Wasm exports (`harmony_alloc`, `harmony_invoke`), the same `export!` macro, the same panels,
effects and app actions apply. A plugin opts in with `"api_version": 2` in its manifest; every
v2 field is ignored by nothing and refused in a v1 manifest, so a v1 package can never reach a v2
feature by accident.

Every API Harmony's own extensions use is the public API described here. The first-party
v2 extensions ([Base64 Decoder](../extensions/plugins/base-decoder),
[Reverse Image Search](../extensions/plugins/reverse-image-search),
[Server Info](../extensions/plugins/server-info) and
[Copy File Contents](../extensions/plugins/copy-file-contents)) and the third-party example
[Quick Notes](../examples/extensions/quick-notes) are built from the published SDK only, go
through the same permission review, and run in the same Wasm sandbox.

## At a glance

| You want to | Declare | Use in the handler |
| --- | --- | --- |
| Let the user configure the extension | `settings` (fields) | `input.setting_bool/text/number(id)` |
| Hold an API key without seeing it | a `secret` setting | `input.secret_set(id)` (presence only) |
| Keep data between runs | `storage` | `kv_set`, `kv_delete`, `kv_get`/`kv_list` requests |
| Read what the user clicked on | `media_links` | `input.media` |
| Read a text attachment | `attachment_contents` | `attachment_read` request |
| Look up a server | `server_profiles` | `server_profile` request |
| Add badges, notes or buttons to the interface | `ui_contributions` and `slots` | `output.contribute(...)` |
| Need a host feature at a version | `requires.features` | refuses to enable otherwise |
| Use a feature only when present | `requires.optional_features` | `host.supports_feature(name, major, minor)` |
| Build on another extension | `dependencies` | enable order is checked by the host |
| Store more than the default | `quotas` | shown to the user, enforced by the host |

Handlers take `ExtensionInvocation` and return `ExtensionOutput`. Both wrap the v1 types, so
everything in the [input reference](extension-sdk-reference.md) and
[action reference](extension-sdk-actions.md) is still there:

```rust
use harmony_extension_sdk::{Element, ExtensionInvocation, ExtensionOutput};

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
    let greeting = input.setting_text("greeting").unwrap_or("Hello");
    ExtensionOutput::panel(vec![Element::Text { text: format!("{greeting}!") }])
}
harmony_extension_sdk::export!(handle);
```

## Manifest fields

All fields below are optional and valid only with `"api_version": 2`.

```json
{
  "api_version": 2,
  "id": "quick-notes",
  "version": "1.0.0",
  "requires": {
    "features": { "storage.kv": "^1.0", "host_requests": "^1.0" },
    "optional_features": { "sync": "^1.0" }
  },
  "dependencies": [{ "id": "notes-core", "version": "^1.2", "optional": false }],
  "conflicts": [{ "id": "old-notes", "version": "<2" }],
  "settings": [{ "id": "max-notes", "label": "Notes to keep", "type": "number", "min": 1, "max": 50, "default": 20 }],
  "quotas": { "storage_bytes": 65536 }
}
```

A v2 manifest's `version` must be a [semantic version](https://semver.org). IDs (extension,
action, setting and button IDs) use lowercase letters, digits and `-`, up to 64 characters.

### Discovery and negotiation

Harmony never requires an exact host version. It publishes what it supports in `HostInfo`
(`input.host()`): `api_versions`, `features` with their semantic versions, `surfaces`, `slots`,
`request_kinds` and the resource `limits`. When the user enables an extension, every
`requires.features` entry must be offered at a matching version, or the extension is refused
with the missing feature named. Missing `optional_features` are listed on the extension's
settings page, and the extension can check them at run time:

```rust
if input.host().is_some_and(|host| host.supports_feature("sync", 1, 0)) { /* … */ }
```

Features offered by this build, all at `1.0.0`: `settings`, `secrets`, `storage.kv`,
`host_requests`, `dependencies`, `media.links`, `attachments.read`, `lookups.server_profile`,
`ui.contributions`.

Packages export `harmony_alloc` and `harmony_invoke`. The SDK's `export!` also exports the same
functions under their earlier names, so a package built today still runs in older releases, and
Harmony accepts packages that export only the earlier names.

A feature's major version changes only when its behavior changes incompatibly; new fields and
request kinds are additive within a major version. The host keeps supporting API version 1.

### Dependencies and conflicts

A required dependency must be enabled in the same account at a version matching its
requirement before this extension can be enabled, and it is checked again every time
extensions load; a broken dependency disables the dependent extension with the reason shown.
Optional dependencies are informational. Conflicts apply in both directions: the host refuses
to enable an extension that conflicts with an enabled one, or that an enabled one conflicts
with. At most 16 dependencies and 16 conflicts.

### Resource quotas

Each extension has its own budget per resource. The defaults are small; `quotas` asks for more,
up to the host maximum, and the user sees the request before enabling.

| Resource | `quotas` key | Default | Maximum |
| --- | --- | --- | --- |
| Settings | `settings_bytes` | 16 KiB | 64 KiB |
| Key-value storage | `storage_bytes` | 1 MiB | 64 MiB |
| Logs (Phase 3) | `log_bytes` | 0 | 256 MiB |
| Media cache (Phase 5) | `media_cache_bytes` | 0 | 512 MiB |
| Network responses (Phase 4) | `network_response_bytes` | 1 MiB | 16 MiB |

All installed extensions together are bounded to 128 extensions and 512 MiB of packages.
A write that would exceed a quota fails the action without changing stored data.

## Settings and secrets

`settings` declares up to 32 fields; Harmony draws the settings page (Settings › Extensions ›
the extension's **Settings** button), validates and saves the values, and passes the current
values with every invocation in `input.settings`, defaults filled in. A value saved by an older
version that no longer fits the field falls back to the default.

| `type` | Value | Extra keys |
| --- | --- | --- |
| `toggle` | `true`/`false` | |
| `choice` | the option's `value` | `options: [{ "value", "label" }]` (1–32) |
| `number` | whole number | `min`, `max` |
| `text` | string | `max_chars` (default 256, at most 4096), `multiline` |
| `secret` | never sent | none; no `default` |

Secrets (at most 8) are typed into a password field and stored in the operating system's
credential store (Windows Credential Manager, macOS Keychain, Secret Service on Linux). If no
credential store is available, secrets cannot be saved; there is no plaintext fallback.
Extension code only learns whether each secret is set (`input.secret_set(id)`); the value is
never sent to Wasm, never written to the extension's folder and never included in storage
exports. Phase 4 adds host-made network requests that can use a secret without exposing it.
Disabling the extension or logging out deletes its secrets.

## Key-value storage

With `storage`, an extension has a private key-value store per account, bounded by its
`storage_bytes` quota. Keys are up to 128 bytes. Writes are part of the output and are applied
atomically after the output is validated:

```rust
output.kv_set("note/00000001", "text");
output.kv_delete("note/00000000");
```

Reads are host requests (next section): `KvGet { keys }` returns each key's value or `None`;
`KvList { prefix, limit }` returns sorted keys (at most 256 per call). The v1 `storage` string
keeps working beside the store. Writes made in a round with requests are applied before the
requests are answered, so a round can delete an entry and list what remains.

## Host requests and continuation

Some information is fetched by the host only when an extension asks for it. A handler returns
`requests` (at most 8 per round, each with an ID unique within the round) and may return an
opaque `continuation` string (at most 8 KiB). The host answers every request and runs the
handler again for the same action with `input.round` increased, the answers in
`input.responses` and the continuation returned. An action may take at most 4 rounds.

A round that returns requests may only also return `kv` writes and `continuation`; the panel,
effects and other output come from the final round. Each request needs a capability:

| `kind` | Capability | Answer (`ResponseValue`) |
| --- | --- | --- |
| `kv_get` | `storage` | `Kv { values }` |
| `kv_list` | `storage` | `KvKeys { keys, truncated }` |
| `attachment_read` | `attachment_contents` | `Attachment { content }` |
| `server_profile` | `server_profiles` | `ServerProfile { profile }` |

A failed request has `error: { code, message }` instead; codes are `not_granted`, `not_found`,
`limit`, `unavailable`, `failed`, `unsupported` and `invalid`. Unknown future codes deserialize
as `Other`.

```rust
match input.response("server").and_then(|r| r.value.as_ref()) {
    None => {
        let mut output = ExtensionOutput::default();
        output.request(HostRequest::ServerProfile { id: "server".into(), guild_id });
        output
    }
    Some(ResponseValue::ServerProfile { profile }) => ExtensionOutput::panel(details(profile)),
    Some(_) => ExtensionOutput::default(),
}
```

* **`attachment_read`** reads the start of an attachment on the message a `message` action was
  chosen on, at most 160 KiB (`max_bytes`), only from `cdn.discordapp.com` and
  `media.discordapp.net`. Valid UTF-8 arrives as `Text`, anything else as `Base64`;
  `truncated` says whether the file is longer.
* **`server_profile`** returns public details of a server the account has joined, fetched with
  `GET /guilds/{id}?with_counts=true` (documented guild object): name, description, image URLs,
  owner ID, creation time, member and online counts, boost level, verification and content
  filter levels, vanity code, locale, feature flags, and counts of roles, emojis and stickers.

## Message media

With `media_links`, a `message` action receives `input.media`: up to 20 public HTTPS links to
the message's attachments and embed images or videos, each with its kind (`image`, `video`,
`audio`, `file`), file name, type, size and dimensions when known. The links are the same ones
the client displays; reading the bytes needs `attachment_contents`.

## Panel buttons of an action

In v1, every panel `Button` must be a declared `panel` action. In v2 a button may also have an
ID that names no declared action. Pressing it runs the **same** action again on the same target
(the same message, menu item or draft), with `input.button` set to the button's ID and the
panel's values in `input.values`. Use it for per-item choices such as "use image 2" or "delete
this note". Declared panel actions keep the v1 behavior.

## Toolbar, image and attachment actions

Three placements put an extension's action where people expect it. Each is drawn by Harmony
with an icon from its set (`icon`), and the action's `label` is the tooltip or menu text.

| `surface` | Where | The handler receives | Needs |
| --- | --- | --- | --- |
| `message_toolbar` | an icon in a message's hover toolbar (up to three extensions) | the message, like `message` | `selected_message` |
| `image` | an entry in an image's right-click menu; a submenu when `choices` are declared | that image as the only `media` link, the chosen entry as `button` | `media_links` |
| `attachment` | an icon on file attachments that `accepts` matches | that file as the only `media` link | `media_links` (and `attachment_contents` to read it) |

```json
{ "id": "search", "label": "Search Image", "surface": "image", "icon": "search",
  "choices": [{ "id": "google", "label": "Google" }, { "id": "all", "label": "All", "icon": "external" }] },
{ "id": "copy", "label": "Copy File Contents", "surface": "attachment", "icon": "copy",
  "accepts": { "extensions": ["txt", "md"], "content_types": ["text/"], "max_bytes": 65536 } }
```

`choices` has at most 12 entries. `accepts` lists lowercase file extensions and content type
prefixes; a file matches either list and must not exceed `max_bytes`.

**Results that apply at once.** When one of these buttons or menu entries is clicked and the
result is only a `copy_text` effect, or only `open_external_link` effects (up to eight), Harmony
applies it right away; the click on Harmony's own button is the consent. A copy shows "Copied to
clipboard" and the button shows a check mark for two seconds. Any other result, or a result with
a panel, opens the usual result window. Actions from other menus always confirm first.

## Panel images

`Element::Image { url, alt, size }` shows an image from Discord's own hosts
(`cdn.discordapp.com`, `media.discordapp.net` and its image proxies), square, 16 to 256 points.
Harmony loads it through its media cache; other hosts are refused.

## Effects

API v2 extensions can copy up to 64 KiB of text with `HostEffect::CopyText` (v1 keeps 4 KiB),
and a v2 result may carry up to eight `open_external_link` effects at once (any other result
carries one effect). Outside the button placements above, the user confirms every effect.

## Permissions

The new capabilities appear in the enable dialog like all others, and enabling still needs
every listed capability:

| Capability | Shown as |
| --- | --- |
| `media_links` | Read links to images, videos and files on messages you choose an action on |
| `attachment_contents` | Read the files attached to messages you choose an action on |
| `server_profiles` | Look up public details of servers you have joined |
| `ui_contributions` | Add badges, notes and buttons to the slots it lists |

## Testing without the app

`harmony_extension_sdk::testing::TestHost` runs a handler through the same JSON path as the
Wasm export, applies `kv` writes, answers requests from fixtures you add (`kv`,
`attachment(...)`, `servers`) and runs continuation rounds up to the host limit:

```rust
let mut host = TestHost::default();
host.servers.insert("10".into(), ServerProfile { id: "10".into(), name: "Test".into(), ..Default::default() });
let output = host.run(input, handle).unwrap();
assert_eq!(host.inputs.len(), 2); // one lookup round, then the answer
```

`TestHost` does not check capabilities or manifest rules; validate the manifest with
`cargo run -p extensions --example manifest_check -- path/to/manifest.json`, and install the
package in Harmony to check permissions.

## Build and package

Build and pack exactly like a v1 plugin:

```sh
cargo build --release --target wasm32-unknown-unknown -p quick-notes
python extensions/pack.py quick-notes/manifest.json target/wasm32-unknown-unknown/release/quick_notes.wasm packages/quick-notes.harmony-extension
```

Import the `.harmony-extension` file in Settings › Extensions. Older Harmony versions refuse a
v2 package with a version error instead of misreading it.

Published v2 packages are listed in `extensions/catalog-v2.json`, separate from the v1
`catalog.json`, so the shop of an older Harmony is never affected by them.

## Compatibility policy

* API version 1 stays supported. New v2 fields are optional and additive; a field is never
  repurposed.
* Host input types in the SDK accept unknown fields, so an extension built today keeps working
  when the host adds fields. Output is validated strictly, so a typo is reported, not ignored.
* A feature's major version is bumped only for incompatible changes, and the previous major
  version is kept for at least one release, with a deprecation note here and in the release
  notes.
* Nothing in this document is available only to Harmony's own extensions.

## Interface contributions

An extension can add small pieces of interface that Harmony draws for it. Declare the
`ui_contributions` capability and every slot you use:

```json
"capabilities": ["ui_contributions", "storage"],
"slots": ["user.badge", "composer.button"]
```

Then return changes in `contributions` from any action, panel button, message or app event,
activation or tick:

```rust
output.contribute(ContributionOp::set(
    Slot::UserBadge,
    "friend-42",                       // your key; other extensions' keys never clash with it
    ContributionTarget::user("42"),
    ContributionItem::text("Friend").color("#3BA55C").icon(ContributionIcon::Tag),
));
output.contribute(ContributionOp::remove(Slot::MessageBadge, "old"));
```

| Slot | Target | Drawn | Text |
| --- | --- | --- | --- |
| `message.badge` | `message` | after the timestamp; under the content on rows that continue the previous message | 32 characters |
| `message.accessory` | `message` | a line under the message content | 300 characters |
| `user.badge` | `user` | after the name in messages and the member list | 32 characters |
| `composer.button` | `app` | an icon button beside the emoji and GIF buttons; runs the declared `panel` action in `action` | 24 characters (accessible name) |

An item has `text`, an optional `tooltip`, a `#RRGGBB` `color`, an `icon` from Harmony's set
(`tag`, `star`, `info`, `warning`, `check`, `sparkle`, `link`, `pin`, `bell`, `shield`) and an
optional `animation` (`pulse`, `shimmer` or `rainbow`, 500–10,000 ms per cycle). Harmony runs
the animation itself and repaints only while animated items exist; your code is never run per
frame. Every item's tooltip names the extension that added it.

**Lifecycle.** Items belong to the extension that set them and are keyed by (slot, key)
within it. `set` replaces an item with the same key, `remove` deletes one, `clear` removes
yours from one slot or all. An optional `ttl_ms` (at most seven days) makes an item expire.
Harmony removes all of an extension's items when it is disabled or fails, and every item when
the account or connection changes. Items are not saved: return them from your `activation`
action to show them again when Harmony loads (see [User Tags](../extensions/plugins/user-tags)).

**Limits and collisions.** At most 64 changes per output, 256 items per slot and 512 per
extension; a batch that would exceed them is refused whole. When several extensions add to the
same message or person, items are ordered by extension ID and then key, so the order never
depends on timing. At most three badges, two lines or four composer buttons are drawn per
target; the rest are summarized ("+2"). A round that returns `requests` cannot also change
contributions.

## Planned (not in this release)

More slots (channel and server list decorations, settings sections) and declarative content
matchers follow the same model. Later phases add hooks, notifications and commands (Phase 3),
host-made network requests that can use secrets (Phase 4), and media services (Phase 5).
