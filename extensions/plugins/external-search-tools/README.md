# External Search Tools

Translate or search a message's text in your browser.

- **Translate** (hover-toolbar button, and in the message menu) opens Google Translate or DeepL
  with the message text, translating into your chosen language.
- **Search message text in browser** (message menu) opens your chosen search engine.
- **Configure external search tools** (Extensions tools menu) picks the translator, target
  language and search engine.

The extension only builds a fixed HTTPS address; your browser does the translation or search,
and Harmony opens the page when you click. Text is trimmed and limited so each address stays
under 1 KiB. Choices are saved locally as three values from fixed lists:

- Translators: Google Translate (default), DeepL.
- Languages: en (default), cs, de, es, fr, it, ja, pl, pt, ru, tr, uk, zh.
- Search engines: DuckDuckGo (default), Google, Brave, Bing, Wikipedia.

It has no network, file or credential access and never sends messages.
