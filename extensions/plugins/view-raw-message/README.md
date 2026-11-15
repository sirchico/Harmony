# ViewRaw

Adds a **View Raw** button to every message's hover toolbar. It opens a **Raw Message Data**
window with the message's exact text under **Message Content** and its public data (IDs and
content, keys sorted) under **Message Data**, plus **Copy Raw Content** and **Copy Message Data**
buttons that copy at once. A setting makes the toolbar button copy the raw text directly instead.

It only receives the text and public IDs Harmony passes to `selected_message` actions; it has no
storage, network or other access. Long text is shown up to 64 KiB and copied up to 64 KiB.
