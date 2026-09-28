# Third-party notices

Hark is MIT-licensed (see `LICENSE`). The following notices apply to bundled
components.

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

## AEC3 Rust port and WebRTC

Hark's optional meeting microphone echo reduction uses **aec3 0.4.0**,
from <https://github.com/RubyBit/aec3-rs/tree/v0.4.0>.
The package manifest declares `MIT OR BSD-3-Clause`; its LICENSE distinguishes
new Rust contributions from WebRTC-derived portions. Both sections and the
accompanying PATENT text are reproduced below from the distributed crate.

### Upstream LICENSE

```text
# WebRTC Derivative Work License and Patent Grant

This repository contains a Rust port of code derived from the WebRTC project.
The original WebRTC source code and its derived elements are subject to the license
and patent grant below.

---

## 1. Copyright for New Contributions

The original code (the Rust implementation) written by Angelos-Ermis Mangos is
licensed under the MIT License.

Copyright (c) 2025, Angelos-Ermis Mangos. All rights reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## 2. WebRTC Project License and Patent Grant

The following license applies to the portions of this software derived from
the WebRTC project source code:

Copyright (c) 2011, The WebRTC project authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.
  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in
    the documentation and/or other materials provided with the
    distribution.
  * Neither the name of Google nor the names of its contributors may
    be used to endorse or promote products derived from this software
    without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Upstream PATENT

```text
Additional IP Rights Grant (Patents)
"This implementation" means the copyrightable works distributed by
Google as part of the WebRTC code package.
Google hereby grants to you a perpetual, worldwide, non-exclusive,
no-charge, irrevocable (except as stated in this section) patent
license to make, have made, use, offer to sell, sell, import,
transfer, and otherwise run, modify and propagate the contents of this
implementation of the WebRTC code package, where such license applies
only to those patent claims, both currently owned by Google and
acquired in the future, licensable by Google that are necessarily
infringed by this implementation of the WebRTC code package. This
grant does not include claims that would be infringed only as a
consequence of further modification of this implementation. If you or
your agent or exclusive licensee institute or order or agree to the
institution of patent litigation against any entity (including a
cross-claim or counterclaim in a lawsuit) alleging that this
implementation of the WebRTC code package or any code incorporated
within this implementation of the WebRTC code package constitutes
direct or contributory patent infringement, or inducement of patent
infringement, then any patent rights granted to you under this License
for this implementation of the WebRTC code package shall terminate as
of the date such litigation is filed.
```
