# Java reference harness

The Rust viewer is checked against the Java/LWJGL `m3g_viewer` it replaces.
`reference-harness.patch` adds the hooks that comparison needs to that
viewer's `src/` tree (it does not change rendering):

* `-Dm3g.dumpDir=DIR` in `Main`: write every batch frame to `DIR/frame_NNNN.png`.
* `-Dm3g.depth=FILE` in `Main`: write the last frame's depth buffer (raw little-endian f32).
* `-Dm3g.only=I,J` in `Renderer`: draw only render objects I and J (after sorting).
* `-Dm3g.dumpmv=1` in `Renderer`: print the GL modelview and projection matrices bit patterns.
* `Dump`, `StateDump`, `WeightDump`, `MorphDump`: scene dumpers (new classes).

Setup (needs a JDK 17+, the original `m3g_viewer.tar.xz` unpacked as `ref_java/`):

    cd ref_java && patch -p1 --directory=. < ../m3g-viewer-rs/tools/java-ref/reference-harness.patch
    ./build.sh

`tests/golden_state.txt` is the output of `StateDump` on the shipped assets and
the generated scenes in `tests/data`.
