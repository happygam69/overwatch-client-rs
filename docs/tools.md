# Tools

Every tool in the project in one place: the question it answers and where it
lives. This is the **only** tool list — `AGENTS.md`, `dev/README.md`,
`docs/README.md` and `docs/provenance.md` point here instead of keeping their
own copy.

All analysis tools are **read-only against the game**: they open the install
and never write to it. The Ghidra project is the working copy, so the naming
and typing tools (`dev/gh_apply_names.py`, `dev/gh_apply_types.py`) do modify
it; `dev/patch_clean_pe.py` writes a patched analysis copy under the
git-ignored `dev/data/offline_recover/`, and never touches the install.
Everything a tool produces — decompiles, hex dumps, corpora, extracted
fixtures — also stays outside the repository.

Run Python tools from the repository root: `python dev/<tool>.py …`. The
tables marked *generated* are built from each tool's own first docstring line
by `python dev/make_index.py`; the gate runs it with `--check` and fails when
a table is stale, so edit the tool's header and regenerate instead of editing
a table by hand.

| I want to … | read |
|---|---|
| check that the tree is healthy | [The gate](#the-gate-cargo-xtask) |
| prove a Rust function equals the client's bytes | [Differential verification](#differential-verification-devverify_harness) |
| read or decompile one client function | [`dev/fn.py`](#the-workhorses) |
| search the whole image | [`dev/corpus.py`](#the-workhorses) |
| ask a question of the install | [Data probes](#data-probes-cratesow-tank) |
| look at the running client | [The client](#the-client-cratesow-client) |
| work in the decompiler | [Ghidra](#decompilation-ghidra) |

## The gate: `cargo xtask`

`crates/xtask` is the project's own command runner (`cargo xtask` is a cargo
alias in `.cargo/config.toml`, so it works on a fresh clone). The gate is what
"done" means: a change is done when `cargo xtask verify` passes and the docs
that describe it are updated.

**The unscoped gate is temporarily suspended by the operator** (seven
install-reading suites dominate its wall clock). Use `--fast` or a scoped run
and say which one was run; see `AGENTS.md`.

| command | what it does |
|---|---|
| `cargo xtask verify` | the full gate: `cargo fmt --check`, the four doc checks, clippy with `-D warnings`, every test target |
| `cargo xtask verify --fast` | the same without the tests that read the install (library targets only) |
| `cargo xtask verify --fail-fast` | stop at the first failing stage instead of running and reporting them all |
| `cargo xtask verify --test-jobs N` | how many test binaries run at once (default `min(4, cores)`; env `XTASK_TEST_JOBS`) |
| `cargo xtask verify -p <crate>` | scope clippy and the tests to the named crates (repeatable, `--package` too) |
| `cargo xtask verify --skip <name>` | leave out a test binary whose label matches (repeatable; e.g. `--skip index_find_real`) |
| `cargo xtask dev-index` | the generated tables in this file match the tools on disk |
| `cargo xtask deviation-check` | `docs/deviations.md` and the code agree |
| `cargo xtask frame-check` | docs cite only the one address frame; the name table and function entries are sane |
| `cargo xtask pe-layout` | each checked address is install bytes, zero-fill or unmapped, as classified |
| `cargo xtask help` | the same list, printed by the binary |

The test binaries are built once (`--no-run`) and run in a parallel pool rather
than walked serially by one `cargo test`, because each install-reading suite
re-opens the install before it starts. Every binary is run even after one
fails, so the log names every failure.

The install-reading suites can also be run on their own:
`cargo test --release -p ow-casc -p ow-stu -p ow-tank -p ow-scene` (the data
layer, the map resolver and the scene) and `cargo test --release -p ow-client`
(the client's own unit tests: camera, movement, HUD, surface).

## Differential verification: `dev/verify_harness/`

The harness proves that a Rust function is the client's function: the oracle
is the client's own bytes emulated offline (from `Overwatch.exe`), the
candidate is Ghidra C (tier 1) or a Rust probe (tier 2). Five stages — load,
determinism, differential, coverage, mutation — bind each verdict to a ledger
entry. `selftest` proves the verifier still detects injected defects.

| command | what it does |
|---|---|
| `python dev/verify_harness/cli.py all` | every unit; stays red while any unit is inconclusive, divergent or invalid (`list` names them) |
| `python dev/verify_harness/cli.py casc` | the verified CASC subset — the green CASC gate |
| `python dev/verify_harness/cli.py fn/<Name>` | one unit, always re-run: naming it is the request to re-prove it |
| `python dev/verify_harness/cli.py fn/A fn/B fn/C` | a batch of units, verified in parallel |
| `python dev/verify_harness/cli.py list` | the unit names and their recorded verdicts |
| `python dev/verify_harness/cli.py selftest` | the verifier's own proof that it catches a defect |
| `python dev/verify_harness/cli.py all --force` | re-prove everything instead of replaying a matching ledger entry |
| `python dev/verify_harness/cli.py all --unit-jobs N` | units verified at once (default `min(4, cpus)`; a unit is never split) |
| `python dev/verify_harness/cli.py fn/X --jobs N` | mutant probes at once per unit (default 4) |
| `python dev/verify_harness/cli.py fn/X --slots N` | cargo build slots per crate (default 4; env `VERIFY_MUTANT_SLOTS`) |
| `python dev/verify_harness/cli.py fn/X --mutation-timeout S` | per-call budget for a mutant probe (default 10 s; the baseline candidate keeps 300 s) |
| `python dev/verify_harness/scaffold_unit.py <address> --name <Name>` | a scaffold for a new client function (autoCorpus on; a hand-written fixture is needed only for linked structures) |

`all` and `casc` replay a ledger entry whose every bound digest still matches
(the oracle window, the candidate, the options, the surfaces, the harness
version and the unit's declared equivalent mutants) and report it as
`(cached)`; anything else, and every unit under `--force`, runs the full
ladder. None of the knobs skips a stage or weakens a gate — they only change
*when* work runs. A unit without an `argFactory` can set `options.autoCorpus`
and the runner grows the seeded corpus until the branch-coverage gate is met
or the cap (default 1200 cases) is reached. A mutant the harness cannot build,
start or drive is retried once; a second infrastructure failure makes the
unit `inconclusive` rather than counting an unobserved mutant as killed, and a
mutant that overruns the doubled call budget on the retry is a
non-terminating mutant and is killed, which preserves the timeout oracle.
The unit format and the five stages are in `dev/verify_harness/runner.py`; the
mutation stage's measured wall time and its quality proof are in
`dev/verify_harness/PERFORMANCE.md`.

### Harness machinery (generated)

<!-- harness-tools:begin -->
| tool | what it answers |
|---|---|
| `dev/verify_harness/batch.py` | One build for a unit's whole mutant set. |
| `dev/verify_harness/candidate_c.py` | Tier-1 candidate: compile a Ghidra decompile, call it through ctypes. |
| `dev/verify_harness/candidate_rust.py` | Tier-2 candidate: run a Rust implementation through a subprocess. |
| `dev/verify_harness/cli.py` | Run verification units. Exit code is the gate. |
| `dev/verify_harness/coverage.py` | Basic-block / branch coverage of the oracle, measured on the very runs |
| `dev/verify_harness/emu_oracle.py` | Unicorn oracle: run one captured client function on one input. |
| `dev/verify_harness/gen.py` | Deterministic input generation for one unit. |
| `dev/verify_harness/ledger.py` | Digest-bound verification records. |
| `dev/verify_harness/mutate.py` | Behaviour-changing mutations of a C candidate, for measuring whether the |
| `dev/verify_harness/runner.py` | The five-stage verification ladder, native edition. |
| `dev/verify_harness/scaffold_unit.py` | Scaffold a verification unit from an address. |
| `dev/verify_harness/selftest.py` | The harness's own gate: inject known defects, require detection. |
| `dev/verify_harness/test_auto_corpus.py` | Auto-corpus growth tests: the autoCorpus option, on synthetic bytes. |
| `dev/verify_harness/test_batch.py` | The batch build: one crate build for a unit's whole mutant set. |
| `dev/verify_harness/test_cache_and_parallel.py` | Fresh-verdict cache, unit-level parallelism, and the ledger lock. |
| `dev/verify_harness/test_cargo_mutant_builds.py` | The cargo candidate path: real crate builds, mutant slots, no staleness. |
| `dev/verify_harness/test_harness_smoke.py` | Smoke tests for the verify_harness fixes. |
| `dev/verify_harness/test_run_unit_e2e.py` | End-to-end test: run the full five-stage ladder on synthetic bytes. |
<!-- harness-tools:end -->

### Unit fixtures (generated)

One fixture per unit that cannot be driven from a flat corpus: deterministic
`(input, output)` cases, so a mutation of the candidate changes the answer.

<!-- harness-fixtures:begin -->
| tool | what it answers |
|---|---|
| `dev/verify_harness/anim_time_wrap_fixture.py` | Structured input factory for fn/AnimTimeWrap. |
| `dev/verify_harness/apply_character_move_fixture.py` | Inputs for the `fn/ApplyCharacterControllerMove` unit (no-shape path). |
| `dev/verify_harness/apply_character_move_steps_fixture.py` | `ApplyCharacterControllerMove` (`0x142cd5370`) as a **steps** trace: the |
| `dev/verify_harness/archive_set_contains_fixture.py` | Deterministic (reader, archive) cases for `fn/ArchiveSetContains`. |
| `dev/verify_harness/blte_block_read_stored_fixture.py` | Deterministic cases for `fn/BlteBlockReadStored`. |
| `dev/verify_harness/box_triangle_fixture.py` | Inputs for the `box_triangle_overlap` unit (`0x142cd58a0`). |
| `dev/verify_harness/build_info_select_row_fixture.py` | Deterministic (rows, flavor) cases for `fn/BuildInfoSelectRow`. |
| `dev/verify_harness/bundle_directory_fixture.py` | Deterministic synthetic bundle directories for the `fn/BundleDirectoryLookup` |
| `dev/verify_harness/bundle_lookup_fixture.py` | Deterministic synthetic bundle payloads for the `fn/BundleLookup` unit. |
| `dev/verify_harness/bvh_fixture.py` | Inputs for the BVH builder unit (`0x142ccb1b0`). |
| `dev/verify_harness/camera_chain_fixture.py` | Inputs for `fn/CameraChainScale` (`0x141ffe590`), and its snapshot. |
| `dev/verify_harness/cdn_index_file_parse_fixture.py` | Deterministic (key, file, len, out) cases for `fn/CdnIndexFileParse`. |
| `dev/verify_harness/cdn_index_footer_fixture.py` | Deterministic inputs for the `fn/CdnIndexFooterParse` unit. |
| `dev/verify_harness/cdn_page_checksums_fixture.py` | Deterministic (file, len, descriptor) cases for `fn/CdnIndexPageChecksums`. |
| `dev/verify_harness/cdn_page_select_batch_fixture.py` | Deterministic (index, keys, keyLen, count, outFound, outLocations) cases |
| `dev/verify_harness/cdn_page_select_fixture.py` | Deterministic inputs for the `fn/CdnIndexPageSelect` unit. |
| `dev/verify_harness/cell_bytes_fixture.py` | Inputs for the `cell_bytes_fixup` unit (`0x141ff51a0`). |
| `dev/verify_harness/character_position_fixture.py` | Inputs for the `fn/CharacterPosition` unit. |
| `dev/verify_harness/chunk_guid_fixture.py` | Deterministic table-slot inputs for the `fn/ChunkGuid` unit. |
| `dev/verify_harness/client_memcmp_fixture.py` | Deterministic (a, b, count) cases for `fn/Memcmp`. |
| `dev/verify_harness/client_strlen_fixture.py` | Deterministic NUL-terminated string cases for `fn/ClientStrlen`. |
| `dev/verify_harness/cmf_iv_154504_fixture.py` | Deterministic inputs for the `fn/CmfIv154504` unit. |
| `dev/verify_harness/cmf_key_154504_fixture.py` | Deterministic inputs for the `fn/CmfKey154504` unit. |
| `dev/verify_harness/collision_capsule_fixture.py` | Inputs for the `collision_capsule` unit (`0x142ccd7b0`, type 1). |
| `dev/verify_harness/collision_capsule_narrowphase_fixture.py` | Inputs for the `collision_capsule_narrowphase` unit (`0x142cfc090`). |
| `dev/verify_harness/collision_filter_fixture.py` | Inputs for the `collision_filter` unit (`0x142cbb350`). |
| `dev/verify_harness/collision_mesh_triangle_fixture.py` | Inputs for the `collision_mesh_triangle` unit (`0x142cae760`). |
| `dev/verify_harness/collision_narrowphase_fixture.py` | Inputs for the `collision_narrowphase` unit (`0x142cc1610`). |
| `dev/verify_harness/collision_runtime_triangle_fixture.py` | Inputs for the `collision_runtime_triangle` unit (`0x142ccf540`). |
| `dev/verify_harness/collision_shape_box_fixture.py` | Inputs for the `collision_shape_box` unit (`0x142cd5c80`). |
| `dev/verify_harness/collision_shape_head_fixture.py` | Inputs for the `collision_shape_head` unit (`0x142cb8950`). |
| `dev/verify_harness/collision_sphere_fixture.py` | Inputs for the `collision_sphere` unit (`0x142ccdf40`, type 0). |
| `dev/verify_harness/collision_sweep_bounds_fixture.py` | Inputs for the `collision_sweep_bounds` unit (`0x142cd5540`). |
| `dev/verify_harness/collision_transform_fixture.py` | Inputs for the `collision_transform` unit (`0x142cb8b80`). |
| `dev/verify_harness/config_key_entry_parse_fixture.py` | Deterministic (key span, value span, vector) cases for |
| `dev/verify_harness/crc32_fixture.py` | Deterministic byte/initial inputs for `fn/Crc32NameHash`. |
| `dev/verify_harness/curve_fixture.py` | Structured input factory for fn/CurveKeySampleDual. |
| `dev/verify_harness/dynamic_index_append_interval_fixture.py` | Deterministic (object, events, entry, resident) cases for |
| `dev/verify_harness/dynamic_index_decode_entries_fixture.py` | Deterministic (entries, indirect, contiguous, initial state) cases for |
| `dev/verify_harness/dynamic_index_entry_sink_fixture.py` | Deterministic (kind, found, chosen, entry, existing events) cases for |
| `dev/verify_harness/dynamic_index_find_entry_fixture.py` | Deterministic (entries, pages, query) cases for `fn/DynamicIndexFindEntry`. |
| `dev/verify_harness/dynamic_index_hash_fixture.py` | Deterministic data/state inputs for the `fn/DynamicIndexHash` unit. |
| `dev/verify_harness/dynamic_index_key_hash_fixture.py` | Deterministic inputs for the `fn/DynamicIndexKeyHash` unit. |
| `dev/verify_harness/dynamic_index_normalize_fixture.py` | Deterministic (object, events) cases for `fn/DynamicIndexNormalizeIntervals`. |
| `dev/verify_harness/dynamic_index_resolve_range_fixture.py` | Deterministic (entries, pages, key, request) cases for |
| `dev/verify_harness/encoding_find_ckey_fixture.py` | Deterministic inputs for the `fn/EncodingFindCKey` unit. |
| `dev/verify_harness/encoding_find_ekey_fixture.py` | Deterministic inputs for the `fn/EncodingFindEKey` unit. |
| `dev/verify_harness/encoding_header_parse_fixture.py` | Deterministic inputs for the `fn/EncodingHeaderParse` unit. |
| `dev/verify_harness/encoding_page_index_search_fixture.py` | Deterministic inputs for the `fn/EncodingPageIndexSearch` unit. |
| `dev/verify_harness/encoding_unnamed_498a70_fixture.py` | Deterministic inputs for the `fn/EncodingEspecStringIndex` unit. |
| `dev/verify_harness/gjk_closest_fixture.py` | Inputs for the `closest_point` unit (`0x142cc9460`). |
| `dev/verify_harness/hex_pair_decode_fixture.py` | Deterministic (out, count, text, len) cases for `fn/HexPairDecode`. |
| `dev/verify_harness/keytable_find_fixture.py` | Inputs for the `fn/KeyTableFind` unit (`0x141b331f0`). |
| `dev/verify_harness/keytable_mask_fixture.py` | Inputs for `fn/KeyTableFindMask` (`0x141b330c0`). |
| `dev/verify_harness/map_chunk_header_fixup_fixture.py` | `MapChunkHeaderFixup` (`0x142e49360`) inputs: a pathing chunk's header |
| `dev/verify_harness/md5_final_fixture.py` | Deterministic context inputs for the `fn/Md5Final` unit. |
| `dev/verify_harness/md5_init_fixture.py` | Deterministic context inputs for the `fn/Md5Init` unit. |
| `dev/verify_harness/md5_transform_fixture.py` | Deterministic block/state inputs for the `fn/Md5Transform` unit. |
| `dev/verify_harness/md5_update_fixture.py` | Deterministic context/input pairs for the `fn/Md5Update` unit. |
| `dev/verify_harness/mesh_fixture.py` | Inputs for the `mesh_sweep` unit (`0x142caed30`, type 3). |
| `dev/verify_harness/mesh_gather_fixture.py` | Inputs for the `mesh_gather` unit (`0x142caea70`). |
| `dev/verify_harness/mesh_shape_fill_fixture.py` | `MeshShapeFill` (`0x142cae540`) inputs: the 0x30-byte source buffer the |
| `dev/verify_harness/move_character_fixture.py` | Inputs for the `fn/MoveCharacterController` unit (no-shape path). |
| `dev/verify_harness/move_character_shape_fixture.py` | Inputs for the `fn/MoveCharacterControllerShape` unit. |
| `dev/verify_harness/mover_apply_modifiers_fixture.py` | Inputs for the `fn/ApplyStateModifiers` unit. |
| `dev/verify_harness/mover_apply_settings_fixture.py` | Inputs for the `fn/ApplyMoverSettings` unit. |
| `dev/verify_harness/mover_command_fixture.py` | Deterministic body inputs for the `fn/BuildMoverPhysicsCommand` unit. |
| `dev/verify_harness/mover_default_settings_fixture.py` | Inputs for the `fn/DefaultMoverSettings` unit. |
| `dev/verify_harness/mover_enter_air_fixture.py` | Inputs for the `mover_enter_air` unit (`0x140ca9150`). |
| `dev/verify_harness/mover_gate_fixture.py` | Inputs for the `fn/MoverGate` unit. |
| `dev/verify_harness/mover_on_ground_fixture.py` | Inputs for the `mover_on_ground` unit (`0x140ca91e0`). |
| `dev/verify_harness/mover_on_landing_fixture.py` | Inputs for the `mover_on_landing` unit (`0x140ca9120`). |
| `dev/verify_harness/mover_on_takeoff_fixture.py` | Inputs for the `mover_on_takeoff` unit (`0x140ca9190`). |
| `dev/verify_harness/mover_property_fixture.py` | Inputs for the `fn/MoverPropertyRecords` unit (`0x140cb3990`). |
| `dev/verify_harness/mover_ramp_fixture.py` | Deterministic inputs for the `fn/MoverRampVector` unit. |
| `dev/verify_harness/mover_ramp_scalar_fixture.py` | Inputs for the `fn/MoverRampScalar` unit. |
| `dev/verify_harness/mover_ramp_scalar_offset_fixture.py` | Inputs for the `fn/MoverRampScalarOffset` unit. |
| `dev/verify_harness/mover_resolve_fixture.py` | Inputs for the `mover_resolve_vector` unit (`0x140cb2220`). |
| `dev/verify_harness/mover_state_fixture.py` | Inputs for the `fn/StateTransform` unit. |
| `dev/verify_harness/mover_state_vector_fixture.py` | Inputs for the `fn/MoverStateVector` unit. |
| `dev/verify_harness/mover_vector_fixture.py` | Inputs for the `fn/MoverVectorRecords` unit (`0x140cb3b10`). |
| `dev/verify_harness/mover_wish_copy_fixture.py` | Inputs for the `fn/MoverWishCopy` unit. |
| `dev/verify_harness/mover_wish_forward_fixture.py` | Inputs for the `fn/MoverWishForward` unit. |
| `dev/verify_harness/mover_wish_inverse_fixture.py` | Inputs for the `fn/MoverWishInverse` unit. |
| `dev/verify_harness/mover_wish_spring_fixture.py` | Inputs for the `fn/MoverWishSpring` unit. |
| `dev/verify_harness/mrnm_fixture.py` | Synthetic `MRNM` payloads for the three model-mesh units. |
| `dev/verify_harness/mrnm_fixup_fixture.py` | `MrnmFixup` (`0x141b893e0`) inputs: a structurally valid MRNM payload with |
| `dev/verify_harness/mrnm_load_fixture.py` | `MrnmHandler` (`0x141b88f10`) inputs: payload (with its relocation slots |
| `dev/verify_harness/mrnm_size_fixture.py` | `MrnmRuntimeSize` (`0x141b89390`) inputs: the same synthetic payloads the |
| `dev/verify_harness/orthonormalize_basis_fixture.py` | Structured input factory for fn/OrthonormalizeCameraBasis. |
| `dev/verify_harness/orthonormalize_state_fixture.py` | State-mode fixture for fn/OrthonormalizeCameraBasis-state. |
| `dev/verify_harness/page_slot_find_fixture.py` | Deterministic (slots, clock, query) cases for `fn/PageSlotFind`. |
| `dev/verify_harness/parse_bool_string_fixture.py` | Deterministic (text, len, flag) cases for `fn/ParseBoolString`. |
| `dev/verify_harness/parse_hex_key_fixture.py` | Deterministic (text, len, out, flag) cases for `fn/ParseHexKey`. |
| `dev/verify_harness/parse_hex_key_id_fixture.py` | Deterministic (text, len, out, flag) cases for `fn/ParseHexKeyId`. |
| `dev/verify_harness/perspective_matrix_fixture.py` | Structured input factory for fn/BuildPerspectiveMatrix. |
| `dev/verify_harness/q12_fixture.py` | Inputs for the Q12 scaler unit (`0x141c6c830`). |
| `dev/verify_harness/query_driver_fixture.py` | Inputs for the `query_driver` unit (`0x142cbd510`). |
| `dev/verify_harness/rigid_position_fixture.py` | Inputs for the `fn/RigidPosition` unit. |
| `dev/verify_harness/salsa20_fixture.py` | Deterministic Salsa20 inputs for the `fn/Salsa20Xor` unit. |
| `dev/verify_harness/salsa20_state_fixture.py` | Deterministic key/nonce/counter inputs for the `fn/Salsa20StateInit` unit. |
| `dev/verify_harness/scope_pop_steps_fixture.py` | Inputs for the `fn/ScopePop-steps` unit (`0x142cee300`), and its snapshot. |
| `dev/verify_harness/segment_aabb_fixture.py` | Inputs for the `fn/SegmentAabbTransform` unit (`0x142697ac0`). |
| `dev/verify_harness/set_velocity_fixture.py` | Inputs for the `fn/SetCharacterControllerVelocity` unit (character path). |
| `dev/verify_harness/shape_box_fixture.py` | Inputs for the `shape_box` shape-bounds unit. |
| `dev/verify_harness/shape_capsule_fixture.py` | Inputs for the `shape_capsule` shape-bounds unit. |
| `dev/verify_harness/shape_dispatch_fixture.py` | Inputs for the `fn/ShapeDispatch` unit. |
| `dev/verify_harness/shape_points_fixture.py` | Inputs for the `shape_points` shape-bounds unit. |
| `dev/verify_harness/shape_pool_alloc_fixture.py` | `ShapePoolAlloc` (`0x142cb00c0`) inputs: a shape pool's record and its |
| `dev/verify_harness/shape_pool_alloc_steps_fixture.py` | `ShapePoolAlloc` (`0x142cb00c0`) as a **steps** trace: the free list walked |
| `dev/verify_harness/shape_sphere_fixture.py` | Inputs for the `shape_sphere` shape-bounds unit. |
| `dev/verify_harness/step_stage_fixture.py` | Inputs for the `controller_step_stage` unit (`0x142cf5020`). |
| `dev/verify_harness/texture_count_bits_fixture.py` | Structured inputs for fn/TextureCountBitsShifted. |
| `dev/verify_harness/texture_descriptor_fixture.py` | Structured inputs for fn/TextureDescriptor. |
| `dev/verify_harness/texture_descriptor_size_fixture.py` | Structured inputs for fn/TextureDescriptorSizeWalk. |
| `dev/verify_harness/texture_format_fixture.py` | Structured inputs for fn/TextureFormatFamilyBpp. |
| `dev/verify_harness/texture_payload_fixture.py` | Structured inputs for fn/TexturePayloadGuid. |
| `dev/verify_harness/texture_request_fixture.py` | Structured inputs for fn/TextureRequestCount. |
| `dev/verify_harness/triangle_fixture.py` | Inputs for the `triangle_degenerate` unit (`0x142cd56e0`). |
| `dev/verify_harness/variant_position_fixture.py` | Inputs for the `fn/VariantPosition` unit. |
| `dev/verify_harness/world_position_fixture.py` | Inputs for the `fn/WorldPosition` unit. |
<!-- harness-fixtures:end -->

## Analysis scripts: `dev/`

One Python script per question, all read-only. The table is the complete list,
and the housekeeping rules at the bottom of this page keep it honest.

### The workhorses

- **`dev/fn.py`** — find and read the client's functions in one command.
  `read <addr>` prints everything known about one function: its name and role
  (`docs/function-names.csv`), the differential unit that tests it and that
  unit's ledger verdict, every place `docs/` already writes it down, the true
  extent (the chain-root union, with the `opaque` flag when the linear listing
  desyncs), the whole-image callers and callees, and the disassembly decoded
  from the image. It also reads a body the unwind table does not cover (the
  client has code no `.pdata` entry describes — `MrnmFixup` `0x141b893e0` is
  one) by running the window to the next known function.
  `packet <addr>` prints everything a reading session needs about one function
  in a single command: the extent and its `.pdata` chain pieces with their own
  sizes, the whole-image callers and callees with their names, the instruction
  shape, the immediates ranked (branch targets excluded, so the real constants
  show), the bounds/tags in `2..0x200`, **switch tables whose entries all land
  inside the body** (the data island that makes a linear listing desync and
  the decompiler time out), every rip-relative data reference with the
  printable string it points at, and whether the client's C is cached. It
  found the 113-entry table and the format-string set behind the reflection
  value reader, and PCRE's compile-option table, each in one command.
  `find <query>` searches by name, by 4CC tag (`MRNM`), by string, by immediate
  or by asset type; a tag is compared by the client as a **big-endian** u32,
  so the tool searches for the immediate and confirms each candidate by
  decoding the function that holds it. `callers`, `callees`, `scope` (the
  transitive closure — the cluster a format's functions form) and `slots`
  (who dispatches through a vtable slot) are queries over the whole-image
  index that `dev/corpus.py index` builds. Everything it needs is the image
  and the corpus index, so it works with no Ghidra session; the corpus index
  is optional and only the whole-image caller/callee queries need it.
  `tags` and `tag <tag>` navigate the name table, and `surface <crate>`
  reports the named client functions that are reachable from what a crate
  cites but that the crate does **not** anchor to an address — the measured
  gap between the client's surface and the crate's, grouped by the crate's own
  files, because a crate file is the format's vocabulary. `jobs` prints the
  client's asset-type registry (type → loader job) **and** the loader behind
  each job, read from the registration that fills it; `jobs --crate <crate>`
  marks the loaders that crate cites. `names` decodes the client's **own
  names** for its assets — every registration function deobfuscates the name
  it registers with the same XOR loop `formats/chunk-kinds.md` documents — and
  ties each name to its type id, its job and its loader.
  `read <addr> --decompile` also runs the client's decompiler and prints the
  C: it drives the project's own headless script (`McpDecompile.java`) against
  the merged working copy and caches the result under `dev/data/decompiles`
  (ignored). The paths are this machine's defaults and the environment
  overrides them — `GHIDRA_HEADLESS`, `GHIDRA_SCRIPTS`, `GHIDRA_PROJECT`. When
  the decompiler cannot run, the tool says **why** (no Ghidra, no script, no
  project, or no function at that address) instead of silently omitting it.
  The merged copy in `GHIDRA_PROJECT` is a separate project file, so the GUI's
  own open project does not lock it.
  `dev/crate_coverage.py` counts an address in either the `0x14…` form or the
  `FUN_…` form a human writes it in; the second form was hidden before and hid
  16 of `ow-tank`'s citations and 13 of `ow-casc`'s.
- **`dev/corpus.py`** — the standing corpus, inside the project at
  `dev/data/corpus` (ignored): `build` a range, `show`/`grep`/`list` it,
  `make-args` for the headless batch, `verify`, `purge` a tag.
  **The unit is a function, not a `.pdata` entry**: entries are grouped by
  their transitive `UNW_FLAG_CHAININFO` root (`dev/fn_extent.py` is the one
  chain walker), giving 119,802 functions over 201,246 entries — chains reach
  8 deep and every union in this image is contiguous, so one `begin end` pair
  is exact. `verify` pins the counts (201,246 / 119,802 / 81,444 / 25,649
  roots) and the zero-gap property on every run; schema 2 of this tool indexed
  each entry, so `db()` migrates such an index from the image (cold halves
  dropped, shortened bodies widened, their decompiles marked `c_cut`).
  Each function gets an authoritative linear disassembly (`.asm`) taken from
  the image, including bodies Ghidra has no function for; Ghidra's decompile
  (`.c`) is added when it exists and is never a verification source. The
  disassembly is a *linear* decode, not a flow decode: a body whose branch
  lands inside its own instruction desyncs, so those are read in the
  decompiler. `show <addr> --window` disassembles around any interior address,
  including a cold half, which resolves through its function's union row; both
  `show` and `grep` flag a hit whose function is `opaque`, where the listing is
  a hint and the decompiler is the read. `build` is incremental (`--force`
  re-fetches decompiles, `--asm-only` refreshes disassembly without the
  bridge). Storage is shared (`asm/<function>.asm`, `c/<entry>.c`); tag
  membership lives in the `tagged` table, so a function can belong to several
  ranges and purging one range never disturbs another's files. `make-args`
  emits unions only and refuses to emit a chained start, because
  `McpDecompile.java` creates a function at any entry Ghidra lacks and creating
  one at a cold half trims its parent; a whole-image range must produce
  exactly 119,802 pairs. The bridge call pins `Overwatch.exe`; a transport
  error is retried and never recorded as a miss.
  `index` builds the whole-image relationship index from the image bytes alone
  (no Ghidra, no decompiler, works while the GUI holds the project lock):
  `fn` (the 119,802-function register, with `opaque` set on the 915 functions
  whose linear listing desyncs), `edges` (1.87M rows: `call`/`jmp`/`jcc` plus
  the rip-relative `FF 15`/`FF 25` API form, each with its site, containing
  function, raw target and form) and `slot_sites` (35,835 `call`/`jmp
  [reg+disp]` dispatch sites with the base register, so "who calls slot +0x40"
  is one query). Every index table is an RVA; attribution is by union span, so
  a call into a cold half or into `.pdata`-less code keeps its raw target, and
  a code transfer that points outside `.text` is dropped and marks its
  function `opaque`. `triage` writes the port scope and state per function
  (forward closure of the gameplay anchors: 3,864 functions, 238 `entry` +
  1,303 `unit` + 2,312 `inline` + 6 `thunk` + 5 `trivial`; see
  `docs/port-plan.md`), and `proto` writes a candidate argument count per
  function from the callee's own entry reads (agrees with 28 of the 44 stated
  unit signatures; a reading aid, never a name table row). `verify` re-checks
  the three documented census facts it must reproduce: the chunk assembler's
  caller at `0x1420251f5` → `0x14209fc90`, the 485 register-slot `+0x40`
  sites, and `ShapeAddToBody`'s 27 sites in 12 functions (the pair that is
  only right when a cold half is not counted as a function).
- **`dev/field_index.py`** — struct-aware field-access index over the complete
  image; separates the mover component, character body and `dmBody`.
- **`dev/pdata_lookup.py`** — `.pdata` function bounds for an RVA.
- **`dev/audit_image.py`** — the ground-truth audit: `.pdata` facts from the
  oracle image, bridge counts, and the name-table reconciliation
  (`--registry` for the full function-registry sweep).
- **`dev/frame_check.py`** — the address-frame gate (`cargo xtask
  frame-check`): docs citations, the name table, and function-entry sanity.
- **`dev/gh.py`** — the Ghidra MCP bridge client used by the naming tools.
- **`dev/gh_apply_names.py`** — applies `docs/function-names.csv` to the
  Ghidra program. `--addr <address>` applies one row (a unit's checklist step;
  the full table is one HTTP call per row and takes minutes), and `--check`
  only reads the project's current names — add `--addr` to check one row.
- **`dev/gh_apply_types.py`** — applies `docs/struct-layouts.csv` to the
  Ghidra program (struct import, then the `prototype` column).
- **`dev/imm_scan.py`, `dev/xref_rva.py`** — immediate and reference searches
  in the current image.

### Every analysis script (generated)

<!-- dev-tools:begin -->
| tool | what it answers |
|---|---|
| `dev/anim_chain_args.py` | Collect the animation-chain function list (main module) for headless. |
| `dev/audit_image.py` | Audit the ground-truth image's function bookkeeping: `.pdata` facts, bridge counts and the name table. |
| `dev/brush_mesh_oracle.py` | Oracle for stage 1 of the brush tree -> mesh conversion. |
| `dev/brush_vertex_oracle.py` | Oracle for the brush's vertex decode, and a probe of the brush builder. |
| `dev/call_slot_scan.py` | Find every `call qword ptr [reg + disp]` site for a set of slot offsets. |
| `dev/callers.py` | Find every static reference to a set of addresses, and name the function it sits in. |
| `dev/chain_placement.py` | Where do `UNW_FLAG_CHAININFO` fragments live, and how far from their parent? |
| `dev/chunk11_probe.py` | Read windows of a chunk asset through `ow-tank-look --hex` and decode them. |
| `dev/clip_dump.py` | Look at a clip section as bytes, in every cheap reading at once. |
| `dev/clip_fp.py` | Byte-level orientation probes for the first-person clip family 0x00020400. |
| `dev/clip_fp_lanes.py` | Column-consistency scanner over the FP-family clip value area. |
| `dev/clip_kinds.py` | Census record kinds across extracted type-0x006 animation clips. |
| `dev/clip_lab.py` | Analysis workbench for the animation clip chunk (asset type 0x006). |
| `dev/clip_layout.py` | Map a clip's reference graph and solve for the element size of each block. |
| `dev/clip_structure.py` | Dump the entire file structure to find pose data. |
| `dev/cluster.py` | Pick and screen a cluster for §9: leaves first, then the head. |
| `dev/codec_scan.py` | Enumerate the client's out-of-line 64-bit obfuscation primitives. |
| `dev/corpus.py` | Build and query a standing corpus for the current image. |
| `dev/crate_coverage.py` | Which client functions does a crate implement, and in what state. |
| `dev/deviation_check.py` | Check that the legacy deviations and the code agree. |
| `dev/disp_scan.py` | Every instruction in `.text` whose memory operand has a given displacement. |
| `dev/eng_build.py` | Build the engine-slot table by walking the disassembly block by block. |
| `dev/eng_bytescan.py` | Byte-level scan of ConstructEngineSubsystems for 64-bit stores with a |
| `dev/eng_class_profile.py` | Profile every subsystem class: its constructor and its vtables. |
| `dev/eng_csv.py` | Produce the engine-slot table from the linear-sweep disassembly. |
| `dev/eng_csv2.py` | Emit dev/engine_subsystems.csv from dev/eng_tab.json plus tail/attach facts. |
| `dev/eng_dump.py` | Linear-sweep disassemble a VA range of the offline image, skipping bad bytes. |
| `dev/eng_filter.py` | Filtered view of the engine dump: keep only decision-relevant instructions. |
| `dev/eng_gen_classes.py` | Write `crates/ow-engine/src/classes.rs` from the constructor/vtable profile. |
| `dev/eng_gen_subsystems.py` | Generate `crates/ow-engine/src/subsystems.rs` from the verified census. |
| `dev/eng_rip.py` | Resolve every rip-relative memory operand in the engine dump and report |
| `dev/eng_tab.py` | Per-block reconstruction of ConstructEngineSubsystems (0x140554220). |
| `dev/eng_table.py` | Reconstruct the engine-slot table of ConstructEngineSubsystems (0x140554220). |
| `dev/eng_verify_schedule.py` | Re-derive ConstructEngineSubsystems' schedule from the instruction stream. |
| `dev/eng_win.py` | Print raw disassembly windows (multiple ranges) from the engine dump. |
| `dev/engine_ref_scan.py` | Find every RIP-relative reference to one or more data addresses in .text. |
| `dev/engine_slot_consumers.py` | Which functions consume which `[engine + slot]`, and what they do with it. |
| `dev/field_index.py` | Field-access index over the complete client image. |
| `dev/find_anim_decoder.py` | Find code that reads the animation clip header, by the loads it must make. |
| `dev/find_field_writes.py` | Find the code that reads or writes one struct-field offset. |
| `dev/find_tables.py` | Find large arrays of code pointers (jump/dispatch tables) in the module image. |
| `dev/fn.py` | Find and read the client's functions in the current image. |
| `dev/fn_extent.py` | Recover a function's TRUE extent when the image stores it as `.pdata` fragments. |
| `dev/fp_table.py` | Decode the client's config-var name table and read each var's default value. |
| `dev/frame_check.py` | Check cited addresses and function-name entries against the one address frame. |
| `dev/gh.py` | Call the Ghidra bridge over HTTP. |
| `dev/gh_apply_names.py` | Name the client's functions in the Ghidra project, and carry the written findings with them. |
| `dev/gh_apply_types.py` | Apply the understood struct layouts and function prototypes to the Ghidra program. |
| `dev/gh_decompile.py` | Decompile one client function by address and write it under dev/decompiles. |
| `dev/gjk_closest_diff.py` | Differential probe: the closest-point GJK, client vs `ow-sim`. |
| `dev/gjk_simplex_diff.py` | Differential probe: the closest-point sub-simplex bodies, client vs `ow-sim`. |
| `dev/gjk_simplex_probe.py` | Oracle probe for the closest-point sub-simplex routines. |
| `dev/gjk_trace.py` | Trace the type-3 triangle test's closest-point GJK over the diff corpus. |
| `dev/gjk_trace_one.py` | Trace the client's closest-point GJK one descriptor at a time. |
| `dev/imm_scan.py` | Scan warm .text pages for 4/8-byte little-endian immediate constants. |
| `dev/ks_hash_test.py` | Test hash-based keystream constructions against the known page stream. |
| `dev/ks_key_test.py` | Test candidate keys/counters against the known page keystream. |
| `dev/ks_linear.py` | Berlekamp-Massey linear-complexity test on the page keystream. |
| `dev/loader_imports.py` | (no header) |
| `dev/make_index.py` | Build the tool tables in docs/tools.md from the tools' own headers. |
| `dev/mesh_support_diff.py` | Differential probe: the type-3 support-point walk, client vs `ow-sim`. |
| `dev/model_class_search.py` | Search the reference STU class definitions for a class whose declared field |
| `dev/model_registry.py` | Enumerate the client's registered class descriptors by their call sites. |
| `dev/offline_page_recover.py` | Recover Overwatch.exe `.text` / `.pdata` offline with the known page cipher. |
| `dev/patch_clean_pe.py` | Build a clean analysis PE: the install's headers plus the recovered .text. |
| `dev/pdata_lookup.py` | Lookup .pdata function ranges containing given rvas. |
| `dev/pe_layout.py` | Classify an address in the install's PE: raw-backed, zero-fill (BSS), or unmapped. |
| `dev/pool_lea_args.py` | Which function collects a given object offset and hands it to a helper. |
| `dev/pool_scan.py` | Verified store/load census for the Domino world's pool-table fields. |
| `dev/probe_candidate_evaluator.py` | Run the fan's candidate evaluator `0x1420328c0` offline with a built world. |
| `dev/probe_cellbytes.py` | Diagnostic: settle MapChunkCellBytesFixup's array/record indirection. |
| `dev/probe_evaluator.py` | Trace one call of an obfuscated function, to map its executed blocks. |
| `dev/probe_node_types.py` | Recover the node-type registry table from the image. |
| `dev/probe_policy_blocks.py` | Recover a function's block graph from the image bytes, and tell a computed |
| `dev/queue.py` | The work queue and the progress number, by tier (`docs/tiers.md`). |
| `dev/rd.py` | Disassemble or hex-dump a virtual-address range of the local image. |
| `dev/rdc.py` | Disassemble a VA range, resolving RIP-relative operands to their float quads. |
| `dev/reach.py` | Which client functions a unit's differential actually executes. |
| `dev/read.py` | One command that prepares a reading session: the evidence pack for a function. |
| `dev/remove_fns.py` | Remove a named set of functions from Rust files. |
| `dev/rip_refs.py` | Find RIP-relative references (lea/mov/cmp, any opcode) to a set of addresses. |
| `dev/slot_call_backscan.py` | Count every register-indirect dispatch that consumes a vtable slot value. |
| `dev/slot_reg_scan.py` | Find vtable-slot dispatches that go through a **register**, not a memory operand. |
| `dev/static_field_scan.py` | Where the client's own code compares an STU field hash or class hash. |
| `dev/static_mesh_consumers.py` | Who builds a Domino shape, and does the builder read a stored mesh? |
| `dev/static_model_schema.py` | Decode a class's field table from its field-table installer function. |
| `dev/static_rva_tables.py` | The image-base-relative 4-byte RVA dispatch tables, and what they select. |
| `dev/static_slots.py` | What a set of client functions read and call, by displacement. |
| `dev/tex_oracle.py` | Cross-check our texture decoders against an independent implementation. |
| `dev/throughput.py` | How much verified behaviour one unit buys, and what it costs per function. |
| `dev/verify_changed.py` | The fast loop: prove only what a change can affect (~30 s, not an hour). |
| `dev/xref_rva.py` | Scan a session image for code references to given rvas. |
<!-- dev-tools:end -->

## Data probes: `crates/ow-tank`

Read-only probes over the operator's installation. Each row is a cargo binary:
`cargo run --release -p ow-tank --bin <binary> -- …` (its own usage line names
it `ow-tank-<binary>`). Examples are
`cargo run --release -p ow-tank --example <name> -- …`.

### Bins

| binary | answers |
|---|---|
| `hero` | a hero's asset and entity STU fields, GUID-typed fields resolved to their target type (`--from 0x…` for any asset, `--fields` for every field) |
| `model` | a model asset: meshes, extents, texcoords, vertex attributes |
| `skel` | which chunk of a model carries the skeleton |
| `modelanim` | model → animation linkage, read from OWLib's own class table |
| `anim` | the animation clip hunt: `--census` installed assets per type, `--scan` assets that name a target instance type, `--walk` asset references from a GUID, `--bones` a clip body's bone tables and float regions |
| `combo` | asset associations at **component** granularity |
| `assoc` | asset-level associations OWLib-style, optional JSON export |
| `look` | follow a map placement's `look` reference and see whether it reaches a texture |
| `light` | lighting manifests (type `0x0BD`): header and chunk list |
| `tex` | textures from the install to PNG, a format census, and the BC6H self-test |
| `shader` | shader assets: the `0x085` group, `0x087` code, `0x086` instance (`0x086` is empty in this build's registry) |
| `sound` | the sound layer: a census of every sound type, the Wwise RIFF header walk on real payloads |
| `replay` | a highlight replay: the `prp` block and the `phl` container around it |
| `content` | the content surface: unlocks, cosmetics, loot boxes, achievements, seasons, conversations |
| `stu` | the typed STU layer: class registry and per-instance decoding |
| `combat` | a per-hero combat table: pools, weapon graphs, every volley literal |
| `params` | statescript graph parameters, by hero or by asset |
| `refwalk` | transitive asset references from a hero or asset, with the graphs and damage nodes reached |
| `verify_probe` | the harness's tier-2 line protocol (internal, not a user tool) |

### Examples

| example | answers |
|---|---|
| `blendset_probe [hero]` | the first-person blend-tree set: items, clips, slots, weights, retarget params, posed arm/weapon positions |
| `blendtree_probe [hero] [item] [tree]` | one blend tree's instances with class/field names and typed values |
| `retarget_probe [hero]` | character vs view-model bind and hierarchy deltas |
| `refskel_probe` | the animation reference skeleton among the install's models |
| `refclip_probe` | a `0x00020400`-family ("first-person") clip through the reference decoder |
| `clip_chan_probe [clip]` | a clip's sampled rotation/position channels against the rig bind |
| `posecheck` | gate: does a decoded clip produce a coherent pose |
| `fpcomp_probe [hero]` | the first-person component and the model component of a hero's view model |
| `hardpoint_probe` | the first-person weapon hardpoints and the bone each resolves to |
| `bonemap_probe [-- --all]` | a mesh's bone indices against the skeleton `Lookup`: the permutation, the size fit, and the bind-distance test that picks the right rule |
| `stu_probe <guid> <instances>` | raw STU payload bytes and the resolved layout of chosen instances |
| `mover_fields_probe` | the mover component's declared field table (offset, hash, …) |
| `collision_chunk_scan` | what the practice range's `MAP_CHUNK_COLLISION` payload actually holds |
| `holder_census` | what the practice range's models carry for collision, by shape mode |
| `map_entity_probe` | the practice range's entity placeables and their component ids |
| `mesh_world_dump` | the range's assembled mesh world, for `dev/probe_candidate_evaluator.py` |

Other crates carry a probe each:

| tool | answers |
|---|---|
| `ow-casc` bin `census` | a read-only byte census of the install's CASC store |
| `ow-casc` example `cdn_fetch` | a live CDN probe for the read-only verification protocol |
| `ow-scene` bin `ground_probe` | the map's ground height sampled along a straight path |
| `ow-render` example `d3d11_smoke` | create the client's D3D11 device and swap chain on a hidden window and run the step list |

## The client: `crates/ow-client`

The one launchable client: practice range by default, assembled through
`ow-scene` and `ow-tank::mapresolve`. A terminal launch asks for a hero; every
non-interactive path takes a flag.

| command | what it does |
|---|---|
| `cargo run --release -p ow-client` | the practice-range eye-check |
| `cargo run --release -p ow-client -- --list-maps` | map headers and variation modes |
| `cargo run --release -p ow-client -- --list-heroes` | the practice mode's roster: names and forward speeds |
| `cargo run --release -p ow-client -- --hero tracer` | spawn with a hero of the mode's roster (name or asset index) |
| `cargo run --release -p ow-client -- --no-hero` | skip the hero prompt |
| `cargo run --release -p ow-client -- --renderer d3d11` | the client's production backend (default is the wgpu viewer) |
| `cargo run --release -p ow-client -- --free-cam` (or `--chunks`) | free camera, no live mover |
| `cargo run --release -p ow-client -- --shot out.png --width 1280 --height 720` | one screenshot, then exit |
| `cargo run --release -p ow-client -- --tour [dir] --count 16` | many viewpoints from one load (three yaws each), then exit |
| `cargo run --release -p ow-client -- --collision level\|models\|both` | which collision world the walker uses (default `level`) |
| `cargo run --release -p ow-client -- --speed 5.5` | the walk's input magnitude in m/s (default: the picked hero's forward speed, else the install-wide 5.5) |
| `cargo run --release -p ow-client -- --spawn X Y Z` | spawn position override |
| `cargo run --release -p ow-client -- --walk-log` | log the live walk's positions |
| `cargo run --release -p ow-client -- --wall-audit` | the wall audit over the collision world, then exit |
| `cargo run --release -p ow-client -- --walk-test` | the walk test over the collision world, then exit |
| `cargo run --release -p ow-client -- --walk-column X Z` | walk a column at `(X, Z)`; add `--render-column` to render it too |
| `cargo run --release -p ow-client -- --frames N` | exit after N rendered frames, printing the frame count and the walker position |
| `cargo run --release -p ow-client -- --d3d11-frames N` | the same limit on the D3D11 backend (presented frames) |
| `cargo run --release -p ow-client -- --no-map` (or `--hero-only`) | the explicit stand-in course: no map; `--hero-only` draws the hero model and nothing else |
| `cargo run --release -p ow-client -- --chunks-from <file>` | an explicit chunk list (one GUID per line, `#` comments); a chunk list is not a map |

`cargo test --release -p ow-tank --test hero_select_real` checks the roster,
names, marker and movement rows against the install.

## Decompilation: Ghidra

The working project is `dev/ghidra_run/owmod_clean` (on the SSD, git-ignored).
The only program is `Overwatch.exe`; [`ghidra-project.md`](ghidra-project.md)
gives the base, the address rule and the workflow. A clean analysis PE (the
install's headers plus the recovered `.text`) is built with
`python dev/patch_clean_pe.py`; the output stays under the git-ignored
`dev/data/offline_recover/`.

Canonical scripts are tracked in [`ghidra/`](ghidra/) and deployed to
`C:/Users/Happy/ghidra_scripts/` (Ghidra only loads scripts from there):

| script | what it does |
|---|---|
| `McpDecompile.java <args-file> <out-file> [timeout-sec]` | the one parameterised decompiler: whitespace-separated `start end` pairs, `#` comments allowed, functions created if Ghidra has none |
| `McpCallers.java <args-file> <out-file>` | callers of each target, with call-site context |
| `FunctionCensus.java` | read-only `.pdata`/function census, for checking a clean import against the live project |
| `DisassemblePdataFunctions.java` | fill the real `.pdata` bodies after a `-noanalysis` clean import |
| `ExportAnnotations.java`, `ApplyAnnotations.java` | move annotations from the pre-clean project to the clean one |
| `CreateMissingNames.java`, `CheckNames.java` | create the no-`.pdata` leaf names, then verify the CSV landed |
| `ImportStructs.java` | apply `docs/struct-layouts.csv` to the program |
| `ConfigureQualityAnalysis.java` | reference/data/string analysis without function-start or body-repair heuristics |

The recovery scripts (`BodySnapshot.java`, `ChainConflicts.java`,
`FixFragmentedPdataFunctions.java`, `FixOuterBodies.java`, `FixOverlapNeighbours.java`,
`MergeChainBodies.java`, `RecoverPdataFunctions.java`,
`RecoverPointerFunctions.java`, `RecoverRealPdataFunctions.java`,
`PdataRepairList.java`, `RecoveryAudit.java`, `ResizePdataFunction.java`,
`DumpFunctionRanges.java`, `ProbeFlow.java`, `ClearFalseNoReturn.java`,
`NoReturnImpact.java`, `NoReturnList.java`, `NoReturnProof.java`,
`DisableNoisyAnalyzers.java`, `ImageAudit.java`, `ImageWorklist.java`,
`DecompScope.java`, `ApplyMoverLabels.java`, `RenameProgram.java`) are the
`.pdata`/`.text` repair pipeline described in
[`ghidra-image-audit.md`](ghidra-image-audit.md) and
[`ghidra-migration.md`](ghidra-migration.md); each is named for the single
repair it performs.

Example:

```text
analyzeHeadless.bat E:\overwatch-client-rs\dev\ghidra_run\owmod_clean Overwatch.exe \
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts \
  -postScript McpDecompile.java E:/overwatch-client-rs/dev/data/corpus/args.txt E:/overwatch-client-rs/dev/data/corpus/out.c
```

## Housekeeping rules

1. A tool stays only if this page lists it (the table is generated from the
   tools themselves) and it still answers a question someone will ask again.
   One-off probes whose answer is already in `docs/formats/` are deleted, not
   parked.
2. A tool that is a superset of another is deleted, and the surviving one's
   header says what it replaced.
3. Every tool's header says what question it answers, in one line. That line
   is the table entry: edit the header and run `python dev/make_index.py`.
4. Outputs larger than about 1 MB go outside the repo.
5. The tools must run from the repository root (`python dev/<tool>.py`).
