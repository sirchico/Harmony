# Text Commands

Literal text replacement rules and reusable message tags for your draft. Edit them from the
Extensions tools menu (**Edit text replacements and tags**); use them from the composer's
Extensions menu. Every change to the draft is a proposal you apply; nothing is sent.

## Replacement rules

A rule has **Find**, **Replace with** and an optional **Only if the draft includes**. Find is
literal and case-sensitive, and it identifies the rule: saving the same Find again updates it.
`\n` in the replacement becomes a new line. **Apply replacement rules** runs each rule once, in
order, each on the result of the ones before it; a condition is checked against that running text.

## Tags

A tag has a name (a lowercase letter, then lowercase letters, digits or hyphens; up to 32) and a
message with placeholders: `{{arg}}` or `{{arg = default}}`. Type `/name` in the composer, with
arguments such as `/greet first name:Ada who:Bob`, and choose **Expand tag in draft**. A new
argument starts at a space followed by another placeholder's `name:`. Missing required arguments,
unknown arguments or unknown tags leave the draft as it is and explain why.

## Limits

Up to 8 rules and 8 tags; each field up to 4,096 bytes; saved data up to 64 KiB; drafts up to
8,000 bytes. Saved data that cannot be read is left untouched and blocks changes until fixed.
It only reads the draft and its own storage; it has no network or message access.
