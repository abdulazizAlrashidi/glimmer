# Local patch to quad-snd 0.2.8

quad-snd is licensed MIT OR Apache-2.0, Copyright (c) Fedor Logachev.

This is the upstream source with a single file changed:
`src/coreaudio_snd.rs`, the macOS backend.

- `BUFFER_FRAMES` goes down from 4096 to 1024 frames per AudioQueue buffer.
- Three buffers are enqueued instead of two.

With the stock settings, output latency on macOS was 93-186 ms. That's
enough to make quick gameplay sounds feel detached from what's happening on
screen. With the patch it's roughly 23-70 ms.

It's wired in through `[patch.crates-io]` in the root `Cargo.toml`.

Upstream's `examples/` folder (about 1 MB of test audio), `.github/` and the
registry's `.cargo-ok` marker were left out of this copy, since none of them
are needed to build the library.
