# Offline Composer Tools

Five tools for your draft, in the composer's Extensions menu. They run entirely offline;
each change is a proposal you apply, and nothing is sent automatically.

- **Clear URL trackers** removes tracking query parameters (`utm_*`, `fbclid`, `gclid`, `dclid`,
  `msclkid`, `mc_cid`, `mc_eid`, `igshid`, `ref_src`, `spm`, `scm`) from http(s) links, keeping
  every other parameter, fragments and the surrounding text.
- **Unindent code blocks** removes the shared indentation inside complete ``` or ~~~ code
  blocks (tabs count as four spaces) and keeps line endings.
- **Convert units** swaps measurements to the other unit: km ↔ mi, kg ↔ lb, cm ↔ in, m ↔ ft and
  °C ↔ °F, with up to two decimals. `10 km` becomes `6.21 mi` and `20°C` becomes `68°F`.
- **Transform text** applies a command at the very start of the draft: `/mock`, `/reverse`,
  `/morse` or `/wordcount`.
- **Character count** shows characters, UTF-8 bytes, UTF-16 units and words.

It only reads the draft; it has no storage, network or message access.
