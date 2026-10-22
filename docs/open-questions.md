# Open questions: the work queue

Every open question in the project, in the order it is worth attacking. Each
entry says what the question is, what would settle it, and where to start. The
state of the project as a whole is in [`../STATUS-handoff.md`](../STATUS-handoff.md);
the per-system state is in [`systems-coverage.md`](systems-coverage.md).

Rules for an entry: a question is *settled* when it has a recorded address or
a measured number, not when it has a plausible guess. A guess becomes a row in
[`fitted-ledger.md`](fitted-ledger.md) and stays a guess.

**Do we have the captures for these?** Yes, with one exception. Every code
address the open questions need is warm in the standing image - measured, not
assumed: [`capture-coverage.md`](capture-coverage.md) lists the address of
each question and whether we hold it. Only question 1 needs a new capture
(it wants runtime state, not code), and questions 2-11 are analysis or
measurement work over code we already have.

---

## 1. The page-key (`w`) derivation - tier 3, declared

**Question.** The on-disk `.text` is XORed with a 4 KB window drawn from a
static 16 KiB cycle. The window's start (`w`) is a build-time uniform draw.
Where is `w` computed?

**What is known.** The XOR transform is exact (2,285 current-build oracle rows
verify). The applier is `0x14380e0` (loop `0x144c839`, caller `0x1015a94`),
live-verified in `cpu_20261021_124450.etl`. The picker is inside the Denuvo VM
(`0x2511e0` dispatcher): the selector chain `0x1ce59b0 -> 0x3fa280 ->
0x1494090` hashes the page VA and takes `hash % count` over a 1,439-entry table
of type-A page records. No stored or hashed `w` table was found in the disk
loader, the live loader, the live main module, the private ghost loader copy
or the full heap dump.

**What would settle it.** A new live capture that is *fault-synchronous*: the
faulting rip and the first handler, in the same session as the `w` value. The
ETL path is tested; the fault path is not.

**Next anchor.** `docs/formats/page-crypt.md` (the runbook, steps 0-5).

**Command.**
`python dev/live_page_diff.py --seconds 90 --out D:/ow-dumps/live_page_diff --all-modules --surge-seconds 90 --include-data --focus-file D:/ow-dumps/live_page_diff/run_20261021_033126/gaps.focus`
then `python dev/faultsync_correlate.py <run>/events.csv` (must print
`ATTRIBUTABLE-CANDIDATE`).

**Impact.** A new build needs a fresh oracle while this is open.

---

## 2. The first-person camera consumer

**Question.** `STUFirstPersonComponent` is read: the per-hero values
(`64/16/-1` then `50/1.25/0.875/0.8`), the schema (`0x70`, 7 fields), the
`m_position` offset, and the consumer path (`FpViewmodelConsumer`, RVA
`0xd2f060` -> bind `0xd2faa0`). The bind stores the references but never reads
`m_position` or the four scalars. Where are they applied per frame?

**What would settle it.** A read of the type-`0x35` system's per-frame path
(the vtable slots `+0xe0` / `+0xe8` are an animation event player, not the
camera). The consumer is a system keyed by type `0x35`/entity offset, still
unread.

**In the current build (2.25.0.1) the path is located and named:** the
consumer is v2b `0x7ff7c5a4f060`, the bind `0x7ff7c5a4faa0`, the two
per-frame slots `0x7ff7c5a51bf0` and `0x7ff7c5a522f0`, the event player
`0x7ff7c5a49c60` - all read from the view-model class's own vtable, all
present in the capture, all named in `docs/function-names.csv`. The
per-frame slot A walks the six 0x110-stride animation slots and runs the
event player on each, which is what the doc says; the camera consumer is
still the open part.

**Next anchor.** `docs/formats/animation.md` (the first-person section) and
`docs/formats/input-camera-audio.md`.

**Impact.** The FP camera framing stays fitted (`--fp-cam-drop`,
`--fp-cam-back`); the eye height and FOV in the ledger are read values but
their application is not.

---

## 3. The Pose2d dataflow VM

**Question.** What drives the first-person blend tree: the driver inputs and
the layer weights. At the authored defaults the tree output is the base clip
(c71).

**What would settle it.** A read of the Pose2d VM in the animation runtime
(the clip/blend/retarget path is measured; the VM that feeds it is not).

**Next anchor.** `docs/formats/animation.md`.

---

## 4. Per-shot damage and armour mitigation

**Question.** Damage is not in the files; it is supplied by the operator.
Damage *order* and armour mitigation are assumptions.

**What would settle it.** A capture of the damage path (the statescript
damage nodes are read, the per-class code is metaobject only).

**Next anchor.** `docs/formats/weapons.md` and `docs/formats/statescript.md`.

---

## 5. Material keys for the blocked assets

**Question.** The model -> look -> texture chain works where the keyring has
the keys. Several hero body materials and many map props are blocked by the
`0x008` keyring wall.

**What would settle it.** A capture of the runtime material structure (the
on-disk shape is not what runs) or a key for the blocked assets.

**Next anchor.** `docs/formats/materials.md`.

---

## 6. The world camera near/far

**Question.** The first-person pair is read (`0.01 / 1638.4 m`); the world
pass still uses the viewer's `0.01 / 8000` stand-in.

**What would settle it.** A read of the world camera's projection setup.

**Next anchor.** `docs/formats/input-camera-audio.md`.

---

## 7. The steep-slide rule

**Question.** Movement is measured and gated by `trace_diff`; the steep-slope
slide is not modelled.

**What would settle it.** A keyed trace on a steep slope, read with
`dev/ctrl_probe.py` / `dev/trace_probe.py`.

**Next anchor.** `docs/formats/character-controller.md`.

---

## 8. The v1 STU path

**Question.** v1 is implemented and synthetic-tested only; no v1 asset has been
read from an install.

**What would settle it.** A v1 asset in an install.

**Next anchor.** `docs/formats/stu.md`.

---

## 9. The replay frame payload

**Question.** The `phl`/`prp` container, header and roster are read; the frame
payload is a Zstd stream that is not decoded.

**Next anchor.** `docs/formats/replay.md`.

---

## 10. Wwise audio decode

**Question.** Wwise metadata and WEM headers are read; the audio itself is
not decoded.

**Next anchor.** `docs/formats/sound.md`.

---

## 11. Bots and abilities

**Question.** They are test stand-ins: boxes and markers, no client AI.

**Next anchor.** `docs/systems-coverage.md` (rows 9-10) and the statescript
docs.

---

## How an entry is closed

1. The answer has a number or an address, and the address names its capture.
2. The finding is written in the relevant `docs/formats/<system>.md`.
3. If it replaces a fit, the row in `fitted-ledger.md` is removed and the code
   says so; if it is still a fit, the row stays and gains the evidence.
4. The entry is removed from this list, and `STATUS-handoff.md` is updated.
