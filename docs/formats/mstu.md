# MSTU: the model's own structured data

`MSTU` is a model asset's structured-data chunk: a two-`i64` header naming the
offset and size of an STU blob inside the chunk's own payload. The blob is a
full STU asset (the reference reads it as an `STUModel`; see `stu.md` for what
an STU holds).

Evidence classes: *decompiled* for the reader shape (TankLib
`teModelChunk_STU`); *install* for the semantics below — measured on
`E:\Overwatch` (build 2.25.0.0.154088) by
`cargo test -p ow-tank --release --test mstu_real -- --nocapture`.

## Layout

```text
0x00  i64 offset   relative to the MSTU payload start
0x08  i64 size     STU blob length from `offset`
0x10  ...          the STU blob (offset 16 on every model inspected)
```

The offset is relative to the *payload*, not the asset. The reference hands
the chunk reader a stream sliced to the chunk's own bytes, and the install
agrees: the STU parsed at `payload[offset..offset+size]` on **60 of 60** models
carrying an MSTU payload, at `asset[offset..offset+size]` on **none**. `offset`
was 16 (its own header) on 60/60. `size` left trailing bytes after the blob on
55 of 60 (5 ended exactly at the payload end), so a reader must slice by `size`
rather than read to the end of the chunk.

First sample, for orientation: `0x0d0000000000001d` — 3,520-byte asset, MSTU
chunk at 2,144, payload 1,376 bytes, `offset` 16, `size` 1,360.

## In the code

`ow-tank::mstu` (`crates/ow-tank/src/mstu.rs`) has `parse_header(asset)` and
`parse(asset) -> ow_stu::Stu`, plus `Model::mstu: Option<MstuHeader>`; the
header is filled by `ow-tank::model::parse` when the chunk is present (a model
without one is not an error). `tests/mstu_real.rs` sweeps the install (62
models scanned, 60 with a payload, 1,951 STU instances total), asserts the
payload-relative reading, and checks the `Model` integration.

**Not tested / open:** what each model's STU *contains* beyond "it parses as
STU" — instance types were not content-checked against the reference's
`STUModel*` classes. A consumer that needs a specific structure (a cloth or
attachment table, say) should assert that structure here first.
