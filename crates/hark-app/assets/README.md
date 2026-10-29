# Embedded assets

All fonts are embedded into the hark-app binary via `include_bytes!` (see
`src/theme.rs`). Provenance (downloaded 2026-07-16, official releases):

| File | Source | License |
|---|---|---|
| Inter-Regular.ttf, Inter-Medium.ttf, Inter-SemiBold.ttf | rsms/inter release v4.1 (`Inter-4.1.zip`, `extras/ttf/`) | SIL OFL 1.1 (`LICENSE-Inter.txt`) |
| JetBrainsMono-Regular.ttf | JetBrains/JetBrainsMono release v2.304 (`fonts/ttf/`) | SIL OFL 1.1 (`LICENSE-JetBrainsMono.txt`) |
| Phosphor.ttf | vendored from the egui-phosphor 0.12.0 crate package (`res/Phosphor.ttf`), regular variant | MIT (`LICENSE-Phosphor-MIT.txt`) |
| Lora-Regular.ttf | [Cyreal Lora](https://github.com/cyrealtype/Lora-Cyrillic/tree/2d53b449b60e185b39f671b44fded83e0910ad30), `fonts/ttf/`, downloaded 2026-09-30 | SIL OFL 1.1 (`LICENSE-Lora.txt`) |
| Alegreya[wght].ttf | [google/fonts `ofl/alegreya/`](https://github.com/google/fonts/tree/6157dfaa640ca5738b4d1a7070fe389c3036d7fc/ofl/alegreya) at commit `6157dfaa640ca5738b4d1a7070fe389c3036d7fc` (`main`, 2026-10-29), downloaded 2026-10-29. sha256 `ba5564634b93a8f8ba57b48cd4f1ae7417d2b4656fbac779028679b00de3cf12` | SIL OFL 1.1 (`LICENSE-Alegreya.txt`, the folder's `OFL.txt`) |
| CinzelDecorative-Regular.ttf, CinzelDecorative-Bold.ttf | [google/fonts `ofl/cinzeldecorative/`](https://github.com/google/fonts/tree/6157dfaa640ca5738b4d1a7070fe389c3036d7fc/ofl/cinzeldecorative), same commit, downloaded 2026-10-29. sha256 Regular `5b862be329103ad287a10f0a53e27a40e8cc519999253f1a0223e2dc330b10b8`, Bold `e854e68a388aa50d742a4415c1ae5c17a617ef7956c95a70021d0a4a44f20518` | SIL OFL 1.1 with Reserved Font Name "Cinzel" (`LICENSE-CinzelDecorative.txt`, the folder's `OFL.txt`) |

Alegreya (Wizard body text) and Cinzel Decorative (Wizard headings and drop
caps) back the optional Wizard lettering. They ship **unmodified**: never
subset, rename or otherwise alter them. Cinzel declares the Reserved Font Name
"Cinzel", and under OFL 1.1 a modified font may not keep that name. Cinzel
Decorative maps neither the no-break space (U+00A0) nor the micro sign (U+00B5);
`theme/tests.rs` pins that gap (`CINZEL_GAPS`), and the Wizard serif family
falls back to Alegreya for those two.

**Weights.** Inter stays as static per-weight TTFs, each registered as its own
font, as before. Alegreya ships as its single variable file instead, and each
weight it needs (400, 500, 600) is a separate registration of the same bytes
pinned with `FontTweak::coords` (`wght`). The old rule here, "egui cannot
interpolate variable-font weight axes" (emilk/egui#1862), predates egui 0.34's
skrifa rasterizer: egui 0.36 applies the tweak's coordinates both when shaping
and when rasterizing (`FontFace::styled_metrics`). The spike test
`alegreya_weights_are_pinned_by_font_tweak_coords` lays out one string at all
three weights and requires the widths to rise (268.8, 270.8 and 272.1 pt at
15 pt on 2026-10-29). If a future egui stops honoring the tweak, that test
fails, and the approved fallback is Spectral's static Regular, Medium and
SemiBold files from the same repository.

Lora provides the editorial Home headline and brand; Inter remains the UI font
(Modern lettering). Its copyright and full license are included in
`LICENSE-Lora.txt`. No fonts are fetched at runtime.

Phosphor is vendored (not a Cargo dependency) because egui-phosphor 0.12.0
pins egui ^0.34 while Hark is on egui 0.35 (checked 2026-07-16). The glyph
codepoint constants in `src/theme.rs` were extracted from the same crate
package's generated `src/variants/regular.rs`, so constants and font cannot
drift. If egui-phosphor ships an egui-0.35-compatible release, switching back
to the crate is a drop-in swap.
