# Third-party notices

Hark is MIT-licensed (see `LICENSE`). It includes the following component under
a different license.

## LAME

Hark encodes meeting audio to MP3 (the compressed archive of a kept recording,
and "Save audio as MP3") with **LAME 3.100**, built from source and linked into
the Hark executable through the `mp3lame-sys` / `mp3lame-encoder` crates.

- License: GNU Lesser General Public License, version 2 or later (LGPL-2.0+).
- Project: <https://lame.sourceforge.io/>
- Source: the LAME source is fetched and built by `mp3lame-sys`; its exact
  version is recorded in Hark's `Cargo.lock`.

As the LGPL requires, you may modify LAME and relink Hark against your modified
version. Hark's complete source is public at
<https://github.com/BoardPandas/Hark>: rebuild it with `cargo build --release`
after pointing `mp3lame-sys` at your modified LAME (for example with a Cargo
`[patch]` entry).
