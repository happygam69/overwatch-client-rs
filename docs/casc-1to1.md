# `ow-casc`: what is verified and what is not

The CASC companion to [`ow-tank-1to1.md`](ow-tank-1to1.md). It records what is
proven against the client's bytes, what is read and tested against the
install, what is ported from a public reference, and what is still open.

## Ground truth

The source is the current build's code. The harness oracle is the installed
`Overwatch.exe` PE with its `.text` decrypted by `dev/offline_page_recover.py`
into `module_offline.bin`: the PE headers and timestamp are the installed
file's own, `.rdata`, `.data`, `.pdata`, `.rsrc` and `.reloc` are byte-identical
to it, and `.text` is the decrypted code. No running process is involved. The
recovery report is `dev/data/offline_recover/REPORT.md` (local, git-ignored);
its held-out check agrees 2,284/2,284.

The unit descriptors carry the recovery image's `v2b` frame. The runner maps it
to the current-image address with
`v2b - 0x7ff7c4d20000 == exe - 0x140000000`; every citation in this page is the
`0x140000000 + RVA` form.

**Gate.** `python dev/verify_harness/cli.py casc` runs the twenty-seven CASC
units (about seven minutes cold; select with a unit name for one). `cargo xtask verify` does **not**
run the differential harness, so a CASC regression can pass the nominal gate.
The ledger binds a verdict to the candidate source and the git revision;
re-run `casc` after a commit if you need a fresh binding.

## A. Verified by the differential harness

| function | address | unit | result | honest caveat |
|---|---|---|---|---|
| `Salsa20StateInit` | `0x1424e3230` | `fn/Salsa20StateInit` | verified: 100%, 24/24 | proves the 64-byte state; the return pointer is not compared |
| `Salsa20Xor` | `0x1424e32c0` | `fn/Salsa20Xor` | verified: 100%, 7/7 scored + 3 declared equivalent | the probe seeks to the counter and advances the state itself; canonical states and counters 0..3 only, high-word carry untested |
| `DynamicIndexHash` | `0x142512c60` | `fn/DynamicIndexHash` | verified: 100%, 12/12 | three post-loop edges declared dead; the client's alignment-dependent loads are oracle-side, the Rust is byte-wise |
| `DynamicIndexKeyHash` | `0x142513230` | `fn/DynamicIndexKeyHash` | verified: 100%, 24/24 | production callers: `DynamicIndexSegmentParse` validates each page record and `DynamicIndexPageInsert` stores the same hash; the page-table read path is implemented |
| `EncodingHeaderParse` | `0x142498920` | `fn/EncodingHeaderParse` | verified: 100%, 39/39 | proves the raw 40-byte struct; the null-data guard is untestable; the production wrapper zeroes the buffer before the call |
| `EncodingFindCKey` | `0x142498540` | `fn/EncodingFindCKey` | verified: 100%, 7/7 | proves the out record address and hit/miss; the page search `0x142498880` is a callee outside the coverage window, measured by `fn/EncodingPageIndexSearch`, and its chosen page is compared here on every case; RAX's upper bits are not compared because callers use AL |
| `EncodingFindEKey` | `0x142498610` | `fn/EncodingFindEKey` | verified: 100%, 10/10 scored + 1 declared equivalent | same split: the search is measured by `fn/EncodingPageIndexSearch`; this unit pins the raw page-search length against the min record-scan length, and the `0xFFFFFFFF` terminator |
| `EncodingPageIndexSearch` | `0x142498880` | `fn/EncodingPageIndexSearch` | verified: 100%, 11/11 | proves the 32-bit miss `0xFFFFFFFF`, the lower-middle `mid`, the `key_len` raw compare (not the record scans' `min`), and that the search writes nothing; a page count that wraps the 32-bit entry offset cannot be represented (the arena cannot hold that index) |
| `EncodingEspecStringIndex` | `0x142498a70` | `fn/EncodingEspecStringIndex` | candidate wired (main session runs the unit): the no-grow string-index build at a 100% both-edge gate | the client's post-read step called only by the encoding open at `0x1424d16c0`: last-byte gate (`block[count-1]`, which is `base-1` when empty) returning 8, a 32-bit NUL count, a reserve to that count, then one walk appending every string start (`base + offset`) after the length helper at `0x1426cce30` returns a non-zero terminator address; `EncodingFile::parse` uses the shipped `espec_string_index` (storing offsets) for its tail check and the 29,694 count. **Declared gaps:** the vector grow at `0x142388210` (allocator vtable at `+0x50`; every case pins count and size+count within capacity, tightest at equality) and a terminator at address 0 |
| `CdnIndexPageSelect` | `0x1424da730` | `fn/CdnIndexPageSelect` | verified: 100%, 14/14 | whole-file mode only (`index+0x38 == 0`, so `CdnIndexPageResident` is pointer arithmetic and no HTTP read runs); the oracle compares AL, `out_offset`, `out_size`, the 16-byte `out_key16`, the whole index object and the key buffer; the streamed resident branch, a null `out_key16` and a page body shorter than the footer describes are declared unreachable/uncovered; the streamed branch is read but not implemented in the port |
| `CdnIndexFooterParse` | `0x1424d9af0` | `fn/CdnIndexFooterParse` | verified: 100%, 12/12 | proves the 0x30-byte footer struct, the six derived dwords, every reject that changes the compared memory, and the AL status (success 0, every reject 8) through the harness's new `u8` return channel, so the final length gate is compared too; the one input the oracle cannot represent is a buffer shorter than 20 bytes (the client reads before the buffer); `allowedImageReads` are the CRT cookie, memcpy flag, a compiler dispatch table and error-format strings |
| `ParseBoolString` | `0x1424d8800` | `fn/ParseBoolString` | verified: 100% (9/9 branches), 10/10 + 2 declared equivalent | the tri-state prefix parser: eight true spellings at `0x143472f10`, eight false at `0x143472f50`, `A..=Z` folding only, a match at either side's end, the flag written only on a match. **The harness's first exe-frame unit** (`address.exe` + `base: 0x140000000`): the tables hold absolute link-time pointers, so the v2b copy faults on the first dereference (`0x143070584`). `install::parse_bool_string` is the port and `parse_bool` is now its caller; the old stand-in's `trim()` and its six-spelling list are gone |
| `BuildInfoSelectRow` | `0x1424aec30` | `fn/BuildInfoSelectRow` | candidate wired (main session runs the unit): the row select at a 90% both-edge gate | `select_build_info_row`/`select_build_info_index` over the table (rows at `+0x20`, count `u32` at `+0x28`, 0xb0-byte rows, Product at `+0`, active at `+0x10`): zero count -> null, null/empty flavor -> no-flavor path (every row must carry the first row's Product, last active wins), otherwise the first active row whose Product equals the flavor byte for byte, case-sensitively; a null Product compares as the empty string (default at `0x142f7aa00`). The shipped `select_build_info` now runs through the same index decision; the correction was the empty-flavor rule (`Some("")` is the no-flavor path). Harness surface is one 0x4000-byte blob plus a u64 flavor address (0 for null), returning the row pointer as an absolute arena address (0 for null). Three count/mismatch retest edges are declared dead |
| `DynamicIndexAppendInterval` | `0x142520700` | `fn/DynamicIndexAppendInterval` | candidate wired (main session runs the unit): the no-grow body at a 20% both-edge gate | `IntervalSet::append` over the key-state object: two events per range (`+1`/`-1` resident, `-0x40000000`/`+0x40000000` non-resident), 64-bit wrapping end, count incremented twice, immediate normalize iff non-resident, record padding never written; harness surface is the 3-arg ABI with obj+events in one 0x2000 blob (reloc at +0x128). **The capacity-grow path is the declared gap** (log `0x1424f8440` + allocator vtable at `[obj]`, new base pointer not byte-comparable; every case pins `capacity > count + 1`); non-resident cases keep the appended pair after every existing start so the normalize sort is a no-op |
| `DynamicIndexDecodeEntries` | `0x1425209a0` | `fn/DynamicIndexDecodeEntries` | candidate wired (main session runs the unit): the 7-arg decode at a 100% both-edge gate | `KeyState::decode_entries` over three sources in order - the 18-byte equal range (one record type 1, more type 2 then type 6), the pointer array's 19-byte records, the contiguous 19-byte records - clearing the count at `+0x130`, preserving found/chosen until a sink changes them, and tail-jumping to the normalize; `SegmentEntries::key_state` now builds its state through it. This unit also corrected `IntervalSet` to the client's shape: a count separate from the physical buffer, so the stale tail a normalize or a count clear leaves is byte-exact. Harness surface is one 0x4000-byte blob plus six u64 arena pointers. **Declared gaps:** the capacity-grow path through the sink (log `0x1424f8440` + allocator vtable; capacity pinned at 256), equal interval starts (the unstable sort's tail is not deterministic), and a live logger for the type 4/5 log call (the fixture's logger is disabled) |
| `DynamicIndexEntrySink` | `0x142520590` | `fn/DynamicIndexEntrySink` | candidate wired (main session runs the unit): the per-type sink at a 100% both-edge gate | `KeyState::sink` over (obj, DL type, 16-byte entry): `dec`/`cmp 6`/`ja` to the default exit, jump table at `0x1425206a8` (type 6 lands inside the type-1 block at `0x1425205d3`); types 1/2 copy all 16 entry bytes and set found (1 also clears the count and appends resident), type 3 clears found/location/size/count but preserves the trailing dword, type 4 clears and appends the current chosen entry resident (after the type 4/5 log call), type 5 clears after the log call, type 6 appends resident, type 7 appends non-resident (immediate normalize). This unit corrected `ChosenEntry` to carry the trailing dword the `movups` copies. Harness surface is one 0x4000-byte blob plus a 16-byte entry pointee (kind as a u64, low byte is the type). **Declared gaps:** the capacity-grow path through the sink, equal interval starts, a live logger for the type 4/5 log call (the fixture's logger is disabled), and sink call sequences (pinned by `fn/DynamicIndexDecodeEntries`) |
| `DynamicIndexFindEntry` | `0x1425111a0` | `fn/DynamicIndexFindEntry` | candidate wired (main session runs the unit): the lower-bound walk, equal-range expansion and page scan at a 90% both-edge gate | `find_entry` over the byte forms (18-byte entries, page table, fresh key state): 32-bit `(lo+hi)>>1` midpoint, `memcmp` 9-byte compare, backward/forward expansion by first-eight-bytes then ninth byte, page scan stopping at the first zero stored hash collecting matching 19-byte payloads in page/record order, then `decode_entries` with the equal range and the collected payloads; the out words are the first zero's `(page, record)`, `(page_count, 21)` for a full table and `(0, 0)` for a zero page count. `SegmentEntries::key_state` now builds its state through `find_entry`. Harness surface is one 0x6000-byte blob (segment, allocator slot, key state, entries, pages, disabled logger, key, out words) plus four u64 arena pointers (key, key state, out page, out record; 0 for a null out word). **Declared gaps:** the pointer-array grow past 32 matches (allocator vtable at `[segment]`, `0x1425113b2`), the interval capacity grow inside the decode (`0x14252073a`), equal interval starts, a live logger for the type 4/5 log call, and general unsorted entry arrays beyond the declared cases |
| `DynamicIndexNormalizeIntervals` | `0x1425208d0` | `fn/DynamicIndexNormalizeIntervals` | verified: 100% (8/8 branches), 12/12 | the interval merge over the client's 16-byte event buffer: sort by start, sum equal-start groups with 32-bit wrap, emit `+1`/`-1` only when coverage crosses zero, compact forward, store the new count at `+0x130`. **Inputs are pre-sorted** (an unsorted buffer's tail is the client sort's own order and `0x14251ff30` is not ported - the declared gap); duplicate starts carry identical bytes so an unstable sort cannot reorder them observably |
| `ParseHexKey` | `0x14248efd0` | `fn/ParseHexKey` | verified: 100% (4/4 branches), 5/5 | the 16-byte text key parser: zero the target and the flag, skip `0x20`/`0x09..=0x0d`, decode 16 bytes through `0x14269ab80`, return `whitespace + 32` or 0. The oracle compares the return, the read-only text, the 16-byte target and the 1-byte flag, so the zeroing, the flag write and the partial writes a short decode leaves behind are all pinned; the decoder's own branches run in the oracle but sit outside the measured window |
| `ParseHexKeyId` | `0x14248f040` | `fn/ParseHexKeyId` | verified: 100% (4/4 branches), 5/5 | the same body with an 8-byte target: `whitespace + 16`. A `count` mutant that would overrun the 8-byte target is killed by a case with 12 hex pairs, and the fixture's 24- and 32-character keys pin the "stop at the target width" edge |
| `Md5Transform` | `0x1426ab530` | `fn/Md5Transform` | verified: 0/0 = 100% (no conditional branch), 18/18 | proves the 16 state bytes after one block and that the block is unchanged; the coverage stage has nothing to measure because the client's 64 rounds are inlined straight-line code, so the mutation stage carries the unit (the RFC 1321 vectors in `blte.rs` pin the same transform through `md5()`) |
| `Md5Init` | `0x1426a5100` | `fn/Md5Init` | verified: 0/0 = 100% (no conditional branch), 6/6 | proves the 0x58-byte context layout (`count` at `+0x00`, the four RFC words at `+0x08..+0x18`) and that the 64-byte buffer at `+0x18` is not touched; the function has no `.pdata` extent, so the coverage window is the measured 36-byte body through its `ret`, and the site is not inferred (44 call sites target it) |
| `Md5Update` | `0x1426a4f40` | `fn/Md5Update` | verified: 100% (4/4 branches), 8/8 | proves the client's four buffering edges, that `fill` comes from the **pre-add** count's low dword while the count advances 64-bit, that whole blocks are transformed straight from the caller's buffer (which must come back unchanged), and that a 0x40-byte data overhang past `len` is never touched; counts are driven across `0xFFFFFFFF` and into a set high word |
| `Md5Final` | `0x1426a5010` | `fn/Md5Final` | verified: 100% (3/3 branches), 9/9 | proves both padding shapes and both "nothing to zero" edges (`fill + 1` of `0x38` and `0x40`), the `count << 3` length word at buffer `+0x38`, and all 16 digest bytes; counts `0xFFFFFFFF` and `1 << 61` pin the wrapping shift; two `<`/`<=` mutants are declared observationally equivalent because each only widens a guard to an empty range |
| `CdnIndexFileParse` | `0x1424d9c90` | `fn/CdnIndexFileParse` | verified: 100% (10/10 branches), 5/5 | the whole-file validator: the footer block (`parse_index_footer`), the terminal `MD5(last keys + checksums)`, `MD5(terminal + footer)` against the 16-byte key, then `CdnIndexPageChecksums`'s status unchanged. The oracle compares AL, the key and file (read-only, so a write through either diverges) and the 0x30-byte derived block, which pins the derived dwords, the footer copy, the hash bytes a header rejection leaves zeroed, and that nothing past `+0x2c` is written. Two inputs are declared gaps: an entry size larger than the page size (the client divides by zero and faults; the port returns 8) and the 32-bit `pageCount*pageSize` / `lastKeyBytes+checksumBytes` wraps |
| `CdnIndexPageChecksums` | `0x1424d9f20` | `fn/CdnIndexPageChecksums` | verified: 100% (4/4 reachable branches of 6 sites, two dead edges declared), 7/7 | proves the status machine (0 all pages, 3 `len` shorter than the page bodies, 8 a stored checksum mismatch), that `bodies = pageCount * pageSize` is a 32-bit product, that the checksum array starts at `lastKeyBytes + bodies`, that whole blocks are hashed through the three verified MD5 stages, and that `len` gates the page bodies only (a mismatch past it is still 8). The mismatch path is a CRT error report that needed a fake TEB for `__chkstk`; the oracle now has one (`HARNESS_VERSION` 6) |
| `ConfigKeyEntryParse` | `0x1424cebd0` | `fn/ConfigKeyEntryParse` | verified: 87.5% (14/16 reachable branches of 21 sites, five dead edges declared), 11/11 + 1 declared equivalent | the `key-<id> = <key>` line: the 32-bit span difference, the token scan bounded by `key_end`, the 24-byte record appended before either parse, `ParseHexKeyId` then `ParseHexKey`, and the disposition (a good id with 32 hex digits returns **1**, a short value returns **0**). The vector grow is the declared gap: it reallocates through the allocator vtable and derives the capacity from eight stack bytes it never initialises, so no input can be compared. Two sites never execute (the grow block's own compare and the token skip loop's back edge) and five edges are dead by construction |
| `DynamicIndexResolveRange` | `0x1425115d0` | `fn/DynamicIndexResolveRange` | verified: 100% (7/7 branches), 5/5 | the whole status machine: 2 no entry, 4 `start > size`, 4 `start + len > size`, the `0xffffffff` length form, the out range written before the resident check (so status 3 carries it), and the entry written only on success and only when its pointer is non-null. The interval grow, the pointer-array grow, the allocator free and a live logger are the declared gaps, the same set `fn/DynamicIndexFindEntry` declares |
| `PageSlotFind` | `0x1424fa440` | `fn/PageSlotFind` | verified: 100% (12/12 branches), 12/12 | the client's single pass: a match (invalid byte clear, archive and page equal, stored covers the length or the slot is busy) ends the pass and is returned even when busy with `out_usable` 0, and only otherwise does the pass pick the largest wrapping `clock - stamp`, skipping busy slots and the excluded one, ties to the lower index. The chosen slot bumps the clock and its stamp. The typed `PageCache::find` + `claim` split is not rewired onto it yet: the caller that consumes the usable answer (`PageCacheRead`, `0x1424fb1c0`) is unread |

| `ArchiveSetContains` | `0x1424fcc80` | `fn/ArchiveSetContains` | verified: 100% (5/5 branches), 9/11 + 2 declared equivalent | the open-archive set membership leaf: the set pointer at `reader+0x238`, the heads array at `set+0`, the splitmix64 finalizer over the zero-extended archive number, the bucket index `((h >> 33) ^ (u32)h) & (bucket_count - 1)` with `bucket_count` the u32 at `set+0x20`, and the bucket chain walk (keys at `+8`, next at `+0x10`, `-1` ends). The two surviving mutants are the first hash shift's ±1 boundaries, declared equivalent: the input is a zero-extended u32, so `h >> 32` and `h >> 34` are both 0 and the first XOR is a no-op. The set's construction and the mode-2 skip are the declared gap |
| `BlteBlockReadStored` | `0x1424d7850` | `fn/BlteBlockReadStored` | verified: 100% (5/7 reachable branches, two dead edges declared), 12/12 | the stored block reader as a streaming copy with a one-byte state flag: zeroes both out counters, status 3 for a null `dst`/`src`, consumes the mode byte when the flag at `+0x10` is non-zero (status 0 for `src_len < 1`, status 8 for a mode other than `N`), status 0 for a zero `dst_len` or an exhausted `src`, otherwise `memcpy` of `min(end - src, dst_len)` through the CRT `0x142efe2b0` and the length added to both counters and the state dword at `+0xc`. The two null-pointer tests are declared unreachable (the harness gives every pointer a non-null arena address). `BlteBlockRead` reaches it through the object vtable slot `[rax + 8]` at `0x1424d668a` |

These are the only CASC functions for which the five-stage word "verified" is
earned: the table's thirty-one rows plus the leaf below. The rest of the
container is not.

`HexPairDecode` (`0x14269ab80`, 318 bytes through the `ret` at `0x14269acbd`)
is the hex-pair leaf under both text key parsers, and `fn/HexPairDecode` is
the thirty-first unit: verified, 100% of its reachable branches, 12/12
mutants, with the shipped `install::hex_decode` as the candidate. Signature
`(u8 *out, u64 count, const char *text, u64 len) -> u64 written`; it stops at
`offset >= len - 1` (an odd trailing character is never read), at a NUL first
character, at a non-hex character, or at `count`, never zeroes `out`, and
leaves its partial output in place. Declared unreachable or uncovered: a null
text pointer, a null `out` (the client has no null check on `out`), and a
`count`/`len` past its buffer (a caller bug).

## B. Read and install-tested, not harness-verified

| area | client address | what is proven today |
|---|---|---|
| `.build.info` row/bool select | `0x14248f2a0`, `0x1424aec30`, `0x1424d8800` | the rules are read-cited and pinned on synthetic text; no install test |
| keyring parse/byte order | `0x1424cebd0`, `0x14248f040`, `0x14248efd0` | read-cited; synthetic byte-order test; the build keyring is read from the install at runtime |
| bucket hash and segment/archive selection | `0x142501b40`, `0x14250e100`, `0x14250e720`, `0x14250e400`, `0x14250c8f0` | the fold is matched on 1,147,132 real keys (salt 0) and 1,488 archive-header placeholders (salt 1); the scan keeps the highest segment per bucket; the archive is `(packed + 0x1e) >> 30`; install-tested, no harness unit (stateful/I/O). **No harness unit for `DynamicKeyStateLookup` either (`0x142501b40`, measured extent `0x142501b40..0x142501c7e`, 318 bytes, v2b `0x7ff7c7221b40`, `(manager, key[9], salt R8B, request, unused, out_range, out_entry) -> status`): read in `docs/formats/casc.md`; it locks the per-bucket mutex (`0x142694c20`/`0x142694c50`) on every path, lazily calls the declared-open `DynamicIndexSegmentOpen` (`0x14250e400`) unless the manager mode is 2 or 4/5, and otherwise runs the manager/segment resolve chain `0x142511540` -> `0x1425115d0` -> `0x1425111a0` (allocator decode context) — no plain-buffer oracle signature. The shipped `index::bucket_of` is the fold only (the client's 32-bit salt add under `& 0xf` is observably the shipped `u8` wrapping add); no probe arm, fixture or unit JSON was wired** |
| index entry search, key-state intervals and resolve | `0x1425111a0`, `0x142511540`, `0x1425115d0`, `0x142520590`, `0x142520700`, `0x1425208d0`, `0x142520c60`, `0x1425209a0` | the client's lower-bound walk, equal-range expansion, sink decode order, interval merge and resident check are implemented; every equal group of all 1,148,620 install entries was range-checked, every salt-0 payload request `(0x1e, size - 0x1e)` resolves status 0 to `packed + 0x1e`, the 1,488 placeholders return status 2, and the 806 duplicate pairs are covered by the second entry's type-6 interval; synthetic tests cover types 1-7, gaps, i32 wrap and all statuses |
| page table layout, hash and lookup | `0x14250fcf0`, `0x1425111a0`, `0x1425209a0`, `0x142513230` | parsed at the client's offset with 21 records per 0x200 page; a record is a u32 hash + key9 + location5 + size4 + type1; the stored hash is `DynamicIndexKeyHash(payload, 0x13, 0)` OR `0x80000000`; the client scans pages and records linearly and stops at the first zero hash. Synthetic non-zero tables cover first/middle/last, type 1/2/3/6, a zero sentinel, a same-key collision and page counts one/many; all 2,560 install pages have zero hashes, so the branch is dormant there |
| archive page cache and `DynamicContainerRead` | `0x1424f48d0`, `0x1424fcd20`, `0x1424fbf00`, `0x1424f8790`, `0x1424faa20`, `0x1424fb1c0`, `0x1424fa440`, `0x1424fa540`, `0x1424fadf0`, `0x1424f9bf0` | read operation for operation: 16 slots of 0x10000 bytes from the `.cmf` open params (`+0x78 = 0x10`, `+0x7c = 0` -> default 0x10000), key `(archive ordinal, page-aligned archive offset)`, no size threshold (the choice is offset/length alignment only), head/middle/tail plan, one direct whole-page run (non-blocking flag 1) and page-served head/tail, whole-page loads, a hit requires the stored length to cover the fragment, the least-recently-used usable slot is reloaded with the lowest index on a stamp tie, a short page zero-fills and reports the short count, an I/O error is status 8 and resets the slot, and status 10 is an archive above 0x3fe or a range crossing a 2^30 boundary. `Store::read_payload` is the client's `(packed + 0x1e, size - 0x1e)` range and `open_ekey` decodes it; `read_stored` is the full-record wrapper for the tests and adds the 30-byte data header with one un-cached read that the client never performs. Install test: small 33,885/33,855 B (stored/payload) is one page read then a cache hit, large 248,182/248,152 B is exactly head page + direct whole-page run + tail page, and the 16 slots still hold 16 after 17 distinct pages with the first evicted. Synthetic tests cover aligned bypass, hit, eviction/reuse, short page, zero length, I/O error and a closed table. No harness unit: the pure plan leaf passed 364 differential cases but its refcount-handle block leaves coverage at 3/6, and the cache itself is lock/I/O state |
| bucket index segment parse/load | `0x14250fcf0`, `0x14250fc60` | read operation for operation: section/file header, section hash, bucket and version/width checks, entry-block bound and rolling hash, page-table offset/minimum and per-record hash, with the client's 5/9 status split. The entry count is `block_size / 18` (floor), so a trailing partial entry is accepted and ignored. The read-only mode-2 path is a synchronous allocator buffer plus one `ReadFile`; the other modes map a read-only view. `ow-casc` reads the same bytes and parses them synchronously. Install-tested on all sixteen files; synthetic tests cover every rejection. **No harness unit for `DynamicIndexSegmentParse` (1240 B, `0x14250fcf0..0x1425101c8`, v2b `0x7ff7c722fcf0`, `(obj, file, mode, bucket) -> status`): the pure validation core at `0x14250ff6a` is mirrored by `parse_bucket_file`, but the prefix that reaches it frees the previous buffer via the allocator vtable (`[rax+0x30]` / `0x1426a8c40`), queries size via IAT `0x14269e180`, and gets the buffer via allocator vtable `[rax+0x18]`/`0x14269e470` or mapped view `0x1426a8d70` (plus `0x142520fa0`, `0x14269dff0`, `0x142521030`, `0x142511f10`, `0x142509420`) — file/allocator/IAT callees outside the system, so no plain-buffer oracle signature exists. Not modelled: writable empty-file creation, the >4 GiB (status 9 cleanup at `0x142510163`, not 0) and mode-2 zero-size edges, and the short-read continuation |
| encoding header/real values | `0x142498920` | header verified; installed 16/16 keys, 4 KiB pages, 16025 + 10519 pages and the 1,484,507-byte espec block are pinned |
| encoding section read | `0x1424d1c60` | read operation for operation: the five sections (espec at `0x16`, then ck index `(ckey_width + 0x10) * ck_count`, ck bodies `ck_page_bytes * ck_count`, ek index `(ekey_width + 0x10) * ek_count`, ek bodies `ek_page_bytes * ek_count`), all offsets and sizes in the client's 32-bit wrapping arithmetic and passed as `u32`, allocator alignment `0x10`, and the range rule `offset > len || (offset == len && size != 0)` with a clamped short read reported as status 14 and a rejected request completing through the no-result lookup as status 3 (the first failing section in layout order; the client merges completion order). The espec string-index build (`EncodingEspecStringIndex`, `FUN_142498a70`, status 8 on an unterminated block) is in section A above. Install-tested on the 111,058,223-byte blob: 16,025 + 10,519 pages, 1,714,571 ckey entries / 1,714,573 ekeys, 1,714,573 espec entries, 29,694 espec strings, all 26,544 page hashes equal MD5 of their page body. Synthetic tests cover every section name in both range branches and the 32-bit wrap. **No harness unit:** the function allocates through a vtable at `*(info[0]) + 0x18`, submits five reads into the container's I/O engine, and takes the container lock; the Unicorn oracle takes plain-buffer arguments and cannot build the container object, its allocator vtable, the TLS/lock state, or the five queued requests. The pure part is the five-offset arithmetic and no client function exposes it separately. The page lookup is no longer format-derived: `EncodingPageIndexSearch` (`0x142498880`), `EncodingFindCKey` (`0x142498540`) and `EncodingFindEKey` (`0x142498610`) are in section A, `EncodingResolve` (`0x142498700`) is read in section B, and the read path performs no page-hash check. **No harness unit for `EncodingHeaderRead` either (`0x1424d1b40`, 281 B, `0x1424d1b40..0x1424d1c59`, v2b `0x7ff7c4f1b40`, `(obj, a, b) -> void`):** it is the refcounted state-machine step that calls the stream-size thunk `0x14248cac0` -> `0x1424904c0`, the lock-cmpxchg acquire `0x14248c9d0`, the verified `EncodingHeaderParse` only when the size is exactly 22, the status merge `0x14249f7a0`, and then tail-jumps through `0x1424d1690` / `0x142490190` and the release vtable `[[obj] + 8]` - allocator/stream/vtable/lock callees outside the system, so no plain-buffer oracle signature exists. `encoding.rs` already models its 8-to-14 remap in `Error::open_status`; see `docs/formats/casc.md` |
| encoding resolve | `0x142498700` | read operation for operation: ready-byte status 1, ckey record's `u32` decoded size, first ekey copy, ekey lookup, ekey `u40` low 32 bits as stored size at `+0x24`, espec pointer through `info+0x58`; the `info+8` multi-ekey path copies the last ekey and returns 7 and is not reached on this install; no harness unit, the two finds prove its parts |
| CDN index file layout and page reader | `0x1424d9af0`, `0x1424d9c90`, `0x1424d9f20`, `0x1424dabb0`, `0x1424dad20`, `0x1424da4c0` | read operation for operation: the 20-byte footer (12-byte header + MD5(header + 8 zeros)[..8]), `pageSize = footer[3] << 10`, `entrySize = footer[4] + footer[5] + footer[6]`, `entriesPerPage = pageSize / entrySize`, `pageCount = (elements - 1 + entriesPerPage) / entriesPerPage` in 32-bit wrapping, `lastKeyBytes = pageCount * keyBytes`, `checksumBytes = pageCount * checksumSize`, and the exact total `checksumSize + 20 + (checksumSize + keyBytes + pageSize) * pageCount`. Tail layout is page bodies, last keys, per-page MD5s, terminal MD5(last keys + checksums)[..checksumSize], footer; page p is `pageSize` bytes at `p * pageSize`; record order is key, size, ordinal (`offsetBytes - 4`), offset, all big-endian. The full-file validator also checks MD5(terminal + footer) against the index's own 16-byte key. Install-tested on all four config-named indexes: archive group 260,173,908 B / 63,149 pages / 9,914,328 entries, patch group 48,616,028 B / 11,800 / 1,923,358, file index 576,828 B / 140 / 28,374, patch file index 90,668 B / 22 / 4,426, every page MD5 matching. The CDN client range-reads the 20-byte footer and the tail and computes full paths via `CdnPathFormat`. |
| CDN index page selection and per-page read | `0x1424da730`, `0x1424da590`, `0x1424b0cc0`, `0x1424da910` | read operation for operation: `CdnIndexPageSelect` lower-bounds the last-key array (`min(keyLen,keyBytes)` compare, early equal exit, `page == pageCount` miss), `CdnIndexPageResident` is pointer arithmetic in whole-file mode or an HTTP range read + full-page + per-page MD5 reject (status 8) in streamed mode, and the record scan caps at `min(entriesPerPage, elements - entriesPerPage*page)` and decodes the size (u32 BE) and offset (u64 BE) by the footer widths; `CdnResolveLocation` splits the 6/5-byte offset into ordinal and offset and falls back to the file index; `ow-casc::container::find_page` follows the expression. A one-off install check (2026-11-05, not a committed test) walked all 63,149 + 11,800 last keys and 606 records across 202 pages; each selects its own page. | `fn/CdnIndexPageSelect` is verified (whole-file mode); the batch wrappers `0x1424b0e50`/`0x1424b0ef0` are expression-level only; the config-to-slot wiring is an inference |
| BLTE frame header and N/Z/E/4 blocks | `0x1424a90e0`, `0x1424d61b0`, `0x1424d6e50` | the client's parse order, `0x0F`/`0x10` records, `12 + record_size*count`, the optional header-MD5 check, the chunk table, the `E` header and Salsa20, and the `N`/`Z`/`4` dispatch are implemented. Synthetic tests cover every reject, both frame types, LZ4, encrypted LZ4 and multi-chunk walking. A one-off full install scan found 1,146,326 unique salt-0 keys: 1,146,325 type-`0x0F` containers, one known non-BLTE orphan, 1,714,597 chunks split `N`/`Z`/`E` = 284,878/1,386,658/43,061, and no `0x10`, direct container or outer `4` block. The install's 63-key keyring lacks the `E` keys (six distinct ids in the 32nd-key sample), so their inner modes are unreadable and an inner `4` cannot be ruled out. The committed test samples every 32nd key and decodes 34 records. Salsa20 itself is harness-verified; the container walk is not |
| `.cmf` header/record/flags | `0x141fda1f0`, `0x141feff80`, `0x141fef610`, `0x141fd4a50` | the flag chain and the store/bundle branch open 305/305 map headers |
| bundle entry lookup | `0x141fd5820` (ow-tank) | `fn/BundleLookup`: verified, 100%, 12/12 |
| bundle directory lookup | `0x141fd5160` (ow-tank) | `fn/BundleDirectoryLookup`: verified, 93.3% with the loaded-bundle false edge as a named gap |
| build-154504 key/IV | `0x142e48a20`, `0x142e48700` | differential + mutation (364 cases each); coverage gate 0, acknowledged gap; not a five-stage verdict |

## C. Ported or reference-derived, not client-read

| area | source | status |
|---|---|---|
| CDN routes/fetch/cache | TACTLib `HttpCDNClient.Fetch` / `ICDNClient` | a port; 12 offline mock tests, no client address, no real endpoint test |
| CMF builds 154088/150851 | TACTLib `ProCMF_154088`; no test for 150851 | ports; only 154504 has differential units |
| config text parse | no client address | format-derived; no test against the install's real config files |

## D. Open

| area | client address | state |
|---|---|---|
| CDN index batch wrappers | `0x1424b0e50`/`0x1424b0ef0` are not Ghidra functions; read at expression level only | `0x1424da910`, `formats/casc.md` |
| CDN HTTP transport | the page request shape is read; the transport is still a TACTLib port with no real endpoint test | `crates/ow-casc/src/cdn.rs` |
| CDN config-to-slot wiring | which writer fills `client+0xf0..+0x110` and `+0x1c0..+0x1e0` is not read | `0x1424b0970` |
| page-table writer | `0x142511d20`, `0x142511c90`, `0x142520cc0`, `0x1426a8ce0` | read operation for operation: 24-byte record at `page * 0x200 + record * 0x18`, first zero hash, payload-then-hash with a full fence, the type/range status machine (0/1/2/4/`0xa`/`0xc`/`0xf`), the eight-page flush at record 20 with `page & 7 == 7`, and the one-retry segment creation above it. **Not implemented:** all five type-3 call sites pass a null range and write location/size from uninitialised stack slots, the writer is allocator and I/O state, and the read path never calls it; all 2,560 install pages are zero, so the positive path is synthetic-only |
| content key = MD5(decoded) | none | measured 300/300; `blte` now has MD5 for the frame header, but the content-key relation has no code or test |

## E. What the docs got wrong (corrected in this pass)

- `docs/formats/casc.md` once said LZ4 mode `4` was implemented when it was
  not. The client's `BlteBlockReadLz4` (`0x1424d6e50`) is now implemented and
  synthetic-tested; no asset on this install uses it.
- `docs/formats/casc.md` called `CmfKey154504`/`CmfIv154504` a "Harness proof"
  although their coverage gate is `0`; they are differential + mutation only.
- `docs/formats/casc.md` and `manifest.rs` called the 154088/150851
  derivations verified; 154088 is a TACTLib port and 150851 has no test.
- The Oracle paragraph said the oracle is `Overwatch.exe`; it is the installed
  PE's decrypted `.text`, which is the same build with all plaintext sections
  byte-identical.
- Three stale numbers: `103 MB` (now 109,882,544 stored / 111,058,223 decoded),
  `1,467,099` (now 1,484,507), and the orphan "357 entry records" sentence.
- `docs/systems-coverage.md` summarised the container as verified while six
  functions are open; the row now separates the verified leaves from the open
  path.
- The CDN page-selection search was recorded as absent from the image. It is
  `CdnIndexPageSelect` (`0x1424da730`); the earlier searches looked for the
  footer offsets `0x14..0x24` (they are at `index+0x6c..0x80`), an immediate
  16 at `memcmp` (the length is a register), a `SHL` stride (it is an `IMUL`),
  and a caller that Ghidra never promoted to a function. `ow-casc`'s
  `find_page` is now the client's expression, not a reference lower bound.
- The docs said no per-page HTTP request call site exists. `CdnIndexPageResident`
  (`0x1424da590`) is one: a range read, a full-page requirement and a per-page
  MD5 reject, reached only in footer-only mode.
- `PageReadPlan` (`0x1424faa20`) was called a pure leaf. It takes the container
  lock twice and AddRefs the archive handle, which is the real reason it has
  no unit.
- The unit notes said the oracle supports four register arguments. It writes
  stack arguments; `fn/EncodingPageIndexSearch` uses a fifth, so the search
  behind both encoding finds is now unit-tested rather than a declared
  coverage gap.
- The bucket section said the client opens "the segment named by the container
  header". There is no such field: `DynamicIndexOpen` (`0x14250e100`) scans
  the directory for `<bucket><segment>.idx`, keeps the highest segment per
  bucket, and caches the sixteen numbers at `+0x110`. The old all-16 merge is
  deleted.
- The page table was described as "a `u32` hash, a 19-byte key and a pad". The
  19-byte payload is key (9) + location (5) + size (4) + type (1); the pad
  makes the record 24 bytes. `DynamicIndexFindEntry` stops at a zero stored
  hash, which is every record on this install; the positive-record path is
  implemented and synthetic-tested, not exercised by the install.
- `docs/casc-1to1.md` said `DynamicIndexKeyHash` had no production caller. It
  has six: `DynamicIndexSegmentParse` validates every page record; the page
  writer (`DynamicIndexPageInsert`) stores the same hash; four other index
  families call it too. The page-table read path is now implemented.
- The page-table writer was listed as the first open read item. It is now read
  operation for operation and documented in section D. It stays unimplemented
  for two reasons: all five type-3 call sites pass a null range and write
  location/size from uninitialised stack slots, and the writer is allocator and
  I/O state that the read path never calls. "Read" is not "implemented", and
  the row says so.
- The segment load was called asynchronous. It is synchronous: mode 2 opens
  the file with `GENERIC_READ` and does one `ReadFile` into an allocator
  buffer; every other mode maps a read-only view. Both paths share the parse
  body. `ow-casc` is a read-only reader, so it follows the mode-2 path with
  `fs::read`; there is no async API to add.
- The entry block's count is `block_size / 18` with no remainder check. The
  old parser rejected a non-multiple block size, which the client accepts; the
  check is removed and a synthetic partial-entry test pins it.
- The encoding page reader was called format-derived. The section *layout* is
  now read operation for operation from `EncodingSectionsRead`
  (`0x1424d1c60`) and install-tested over the whole blob, including the
  client's 32-bit wrapping arithmetic and the espec tail check. The *lookup*
  (binary search, record scan, page-hash check) is now read too: the search is
  `EncodingPageIndexSearch` (`0x142498880`), the two record scans are
  `EncodingFindCKey`/`EncodingFindEKey`, and the resolve is `EncodingResolve`
  (`0x142498700`). The read path performs **no page-hash check**; the stored
  16-byte hash is only the search stride's padding. The first sampled record
  also corrected the two sizes: the ckey record's `u32` is the decoded size
  (524,304 here) and the ekey record's `u40` low 32 bits are the stored size
  (3,530 here), not the other way round. The section reader itself still has
  no harness unit because it is allocator and I/O state: there is no
  plain-buffer signature for the Unicorn oracle to call.
- `DynamicContainerRead` was said to have no page cache. The page reader and
  its 16-slot/0x10000-byte cache are now read operation for operation  (`0x1424fcd20`, `0x1424fbf00`, `0x1424f8790`, `0x1424faa20`,
  `0x1424fb1c0`, `0x1424fa440`, `0x1424fa540`, `0x1424fadf0`,
  `0x1424f9bf0`), implemented in `crates/ow-casc/src/pagecache.rs`, and
  install-tested. The geometry comes from the `.cmf` database open params, not
  the index header. `Store::read_payload` now performs the client's exact
  `(packed + 0x1e, size - 0x1e)` range through the cache; `Store::read_stored`
  keeps its full-record contract by adding the 30-byte data header with one
  un-cached read, which the client never does. The old cross-archive loop was
  replaced by the client's status-10 rejection for a range whose first and last
  bytes are not in the same archive.
- `PageSlotPublish` (`0x1424f9bf0`) is now read in full (measured extent
  `0x1424f9bf0..0x1424f9e3f`, 591 bytes past its 14-byte `.pdata` root,
  signature `(table, slot, stored u32, flag u8) -> void`, v2b
  `0x7ff7c7219bf0`): it walks the slot's waiting keys through the table's
  hash buckets and publishes each matching head/tail fragment (full-length
  copy from `slot + 0x18 + in_page`, wrapping served count, zero-fill past
  it, flag store, slot word cleared first), then writes the slot's stored
  count, `0xff` fill, waiting count, busy byte and conditional key clear in
  that order, and tail-jumps through `0x14269eae0` on `slot + 0x68`. The
  shipped `copy_fragment` was corrected to the full-length copy and
  wrapping served count (it previously saturated `stored - in_page` to
  zero when `stored < in_page`); the hash/waiting-key publish, slot
  post-writes and tail release are not in the shipped synchronous
  `PageCache`. It cannot be a five-stage unit - the unconditional
  IAT tail-jump at `0x142f2d5f8` holds `0x3843c96` in the offline image and
  faults in the oracle - so it is declared open in `docs/formats/casc.md`
  with no probe, fixture or unit JSON, not faked.

- The CDN index footer was called a 12-byte footer with the checksum size
  searched in 8..=16 and `pageCount` derived from the footer's position. The
  client's `0x1424d9af0` reads a fixed **20-byte** footer, takes the checksum
  size from `footer[7]`, and derives `pageCount` from the element count:
  `ceil(elements / (pageSize / entrySize))`. The footer also carries
  `MD5(header[0..12] + 8 zero bytes)[..8]`; the full-file validator
  `0x1424d9c90` checks the terminal `MD5(last keys + checksums)`, the total
  length, and `MD5(terminal + footer)` against the index's 16-byte key. The
  reference's position-based formula happened to equal the client's on these
  four files; it is not the client's expression and is gone.

- `blte.rs`'s `md5()` named `Md5Init`/`Md5Update`/`Md5Final` in its doc comment
  and described their work, but the code was one pass over the whole input: the
  client's 0x58-byte context and its `count & 0x3f` buffering rule were not
  modelled, because a whole buffer in memory produces the same digest either
  way. All three stages are now ported at the client's own offsets and verified
  (`fn/Md5Init`, `fn/Md5Update`, `fn/Md5Final`), and `md5()` is their
  composition, so the RFC 1321 vectors now run the client's staging. This also
  corrects the earlier claim that `Md5Transform` was the only MD5 function with
  a unit: four of the five have one (`Md5Transform`, `Md5Init`, `Md5Update`,
  `Md5Final`), and the three new ones read the context layout the frame-header
  hash and the CDN index validators share.

## Next order

The segment parse/load and the page-table writer are no longer read items;
section B records the parse/load, and section D records the writer with the
reason it stays unimplemented. The encoding section read and the archive page
cache (`DynamicContainerRead`'s `0x1424f48d0`/`0x1424fcd20` path) are now in
section B; the encoding page lookup and the CDN index footer are in section A.
The remaining read order is:

0. **MD5, the page check, the whole-file parse and the text key parsers are
   closed.** `Md5Init`/`Md5Update`/`Md5Final`, `CdnIndexPageChecksums`
   (`0x1424d9f20`, 372 B), `CdnIndexFileParse` (`0x1424d9c90`, 642 B),
   `ParseHexKey` (`0x14248efd0`, 102 B) and `ParseHexKeyId` (`0x14248f040`,
   106 B) are all in section A. `CdnIndexPageResident` (`0x1424da590`, 401 B)
   is the one the print order names next and it **cannot** be a five-stage
   unit: its streamed branch is an HTTP range read (`0x142522870` start,
   `0x142522380` wait) plus a per-page MD5 of the returned page, so those
   branch sites cannot run in the oracle, and its whole-file branch is two
   stores already exercised through `fn/CdnIndexPageSelect`, which calls it. It
   stays read and documented. The next unit in the order is
   `BlteFrameHeaderParse` (`0x1424a90e0`, 982 B), which calls the now-verified
   `Md5Update`. The batch path below is the remaining read item.

1. **The CDN index batch path.** The single-key selection (`0x1424da730`),
   the per-page read (`0x1424da590`) and the resolver (`0x1424b0cc0`) are read;
   the two batch bodies `0x1424b0e50`/`0x1424b0ef0` were never promoted to
   functions and are read only at the expression level. Their
   operation-for-operation read is the remaining read item, followed by the
   config-to-slot wiring. The single-key path is verified by
   `fn/CdnIndexPageSelect` (whole-file mode, 14/14 mutants).

What is still not tested is listed where it belongs: the page-table writer is
read but not implemented (section D: null ranges and uninitialised stack
fields at every type-3 call site, and no read-path caller); the CDN index batch
wrappers are expression-level only; the CDN HTTP transport is a TACTLib port with no real endpoint test
(section C); BLTE `0x10` is absent from this install, no
outer `4` block was observed, and the `E` inner modes are unreadable because
the 63-key keyring lacks the keys; the CDN index `fn/CdnIndexFooterParse` unit
now compares the client's AL status through the harness's `u8` return channel
(success 0, reject 8), so the final length gate is no longer an unobservable
gap; and install measurements that are not test-pinned stay measurements.

The install-wide byte census (`cargo run --release -p ow-casc --bin census`, a
full stride-1 scan of 1,146,326 records, frame headers and mode bytes only) is
measured: 84,463,293,804 index bytes hold 84,373,987,087 chunk bytes, split
`N`/`Z`/`E` = 8,858,748,676/72,530,821,791/2,984,416,620 stored bytes
(10.50%/85.96%/3.54%). The 43,061 `E` chunks are 3.54% of chunk bytes and
3.42% of decoded bytes; their 11 distinct key ids are absent from the 63-key
keyring, so **22,506 records (1.96%) cannot be decoded at all**. `--stride N`
reproduces a sample, and `crates/ow-casc/tests/blte_real.rs` asserts that the
bytes classified across modes equal the frame headers' declared total.

The page-cache plan (`PageReadPlan`, `0x1424faa20`) is not a pure leaf: its
body does the atomic refcount AddRef/Release on the archive handle
(`lock inc [r8+8]` / `lock xadd [rcx+8],eax` at `0x1424faa76`/`0x1424faa89`)
and calls the handle's release vtable slot `*(handle)+8` at `0x1424faa97`
when the count drops to 1. That release is why its unit cannot earn the
five-stage word (coverage 3/6; a branch that never ran cannot be declared
dead). The unit was removed rather than shipped with a weakened gate.

**The same wall, measured (2026-11-17).** Three more page-cache entry points
are read and cannot be units for the same reason: `PageCacheInvalidateAll`
(`0x1424fab50`) and `PageCacheInvalidateRange` (`0x1424fabd0`) call the client's
mutex through `0x142694c20`/`0x142694c50`, `PageCacheClose` (`0x1424f9eb0`) and
`PageCacheInit` (`0x1424f8790`) sit on the same object, and every one of them
would have to execute that lock inside the oracle. `PageSlotFind`
(`0x1424fa440`) is lock-free; its single pass was shipped as
`pagecache::slot_find` and it is now `fn/PageSlotFind` (2026-11-19).

**Two more read walls.** `BlteFrameHeaderParse` (`0x1424a90e0`) allocates the
chunk table through three IAT calls (`0x1424a92a4`, `0x1424a9301`,
`0x1424a936c`, import slot `0x143aafdc9`; the type-`0x10` extras through
`0x143aafdd0`), and the oracle maps no heap. `ConfigKeyEntryParse`
(`0x1424cebd0`) grows its entry vector through an allocator vtable
(`[rax+0x10]` at `0x1424cec83`, free at `0x1424cecaf`) **when the count equals
the capacity**; every other path is pure, so it is `fn/ConfigKeyEntryParse`
(2026-11-19) with the grow as that unit's declared gap. `BlteFrameHeaderParse`
still cannot be a unit: its success path allocates unconditionally.

**One more read wall (2026-11-18).** `DynamicIndexSegmentOpen`
(`0x14250e400`, measured extent `0x14250e400..0x14250e58c`, 396 bytes, v2b
`0x7ff7c722e400`) is read in `docs/formats/casc.md` and cannot be a
five-stage unit: its cache-hit returns (0 already open, 1 cached greater)
are pure, but the open path it falls into is file/CRT/IAT state - the name
build through `0x14269acd0`/`0x1426a1bb0`, the file open through
`0x14269e1c0`, handle transfer through `0x14269dd30` - and it ends in
`DynamicIndexSegmentLoad` (`0x14250fc60`), itself declared open, whose
return it discards. The oracle cannot run the open-success, open-error and
load branches, so coverage and mutation cannot be earned; no probe arm,
fixture or unit JSON was wired for it. The shipped `index::segment_name`
remains the formatter only, matching the client for `bucket < 16`.

**One more read wall (2026-11-18).** `DynamicKeyStateLookup`
(`0x142501b40`, measured extent `0x142501b40..0x142501c7e`, 318 bytes, v2b
`0x7ff7c7221b40`) is read in `docs/formats/casc.md` and cannot be a
five-stage unit: the per-bucket mutex (`0x142694c20`/`0x142694c50`) runs on
every path, the lazy segment open is the declared-open
`DynamicIndexSegmentOpen` (`0x14250e400`), and the resolve chain
(`0x142511540` -> `0x1425115d0` -> `0x1425111a0`) is manager/segment state
with an allocator decode context. Pinning the manager mode to 2 or 4/5 would
skip only the open call. No probe arm, fixture or unit JSON was wired for
it; the shipped `index::bucket_of` is the bucket fold only, and the
found/chosen behaviour is pinned by `fn/DynamicIndexDecodeEntries` and
`fn/DynamicIndexEntrySink`.

**One more read wall (2026-11-18).** `ContainerReaderInit` (`0x1424fbf00`,
measured extent `0x1424fbf00..0x1424fc09a`, 410 bytes, v2b `0x7ff7c721bf00`)
is read in `docs/formats/casc.md` and cannot be a five-stage unit. The
requested `BuildInfoRowsAdvance` does not exist at this address: the body is
the container-reader initializer (reader in RCX, returned in RAX; the caller
at `0x1424f4294` stores it at `container+0xb38`), not a row-cursor advance,
and there is no shipped row-cursor candidate in `install.rs` to check
against. On every path it calls `PageCacheInit` (`0x1424f8790`), the
handle-map init `0x14250c7b0`, and the mutex unlock `0x142694c30`; the
non-null path adds an indirect vtable call (`[rax+0x18]`) and string/memory
helpers. No probe arm, fixture or unit JSON was wired for it; it stays read
and open, not faked.

## Closing a row

A row moves to verified only when a unit names the current-image address, uses
a `candidateCrate: "ow-casc"` probe, and passes all five harness stages. A row
may instead be recorded as a port or an open question; it may not simply
disappear.
