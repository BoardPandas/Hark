# Embedded assets

All fonts are embedded into the hark-app binary via `include_bytes!` (see
`src/theme.rs`). Provenance (downloaded 2026-07-16, official releases):

| File | Source | License |
|---|---|---|
| Inter-Regular.ttf, Inter-Medium.ttf, Inter-SemiBold.ttf | rsms/inter release v4.1 (`Inter-4.1.zip`, `extras/ttf/`) | SIL OFL 1.1 (`LICENSE-Inter.txt`) |
| JetBrainsMono-Regular.ttf | JetBrains/JetBrainsMono release v2.304 (`fonts/ttf/`) | SIL OFL 1.1 (`LICENSE-JetBrainsMono.txt`) |
| Phosphor.ttf | vendored from the egui-phosphor 0.12.0 crate package (`res/Phosphor.ttf`), regular variant | MIT (`LICENSE-Phosphor-MIT.txt`) |

Static per-weight TTFs are deliberate: egui cannot interpolate variable-font
weight axes (emilk/egui#1862), so each weight registers as its own font
family.

Phosphor is vendored (not a Cargo dependency) because egui-phosphor 0.12.0
pins egui ^0.34 while Hark is on egui 0.35 (checked 2026-07-16). The glyph
codepoint constants in `src/theme.rs` were extracted from the same crate
package's generated `src/variants/regular.rs`, so constants and font cannot
drift. If egui-phosphor ships an egui-0.35-compatible release, switching back
to the crate is a drop-in swap.
