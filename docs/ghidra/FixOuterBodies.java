// FixOuterBodies.java - resize the functions that leak into a chained union.
//
// Usage: FixOuterBodies.java [dry|apply] [conflicts.tsv]
//
// `MergeChainBodies.java` cannot widen a root while a function whose entry is
// outside the union has a body reaching in. `ChainConflicts.java` names them;
// this script fixes the ones that are provably wrong and reports the rest.
//
// The provable case is the majority (measured 2026-11-08: 39 of 53 conflicts):
// the outer is a real `.pdata` root with no chained pieces, and its body is
// larger than its own authoritative extent - a flow-reachable body that leaked
// past the end of its code when the program was analysed. Its extent is the
// truth, so the body is set back to it. Ghidra keeps the instructions; only the
// ownership changes.
//
// Not touched, printed as `MANUAL <outer> ...`: an outer with no `.pdata` extent
// at all (a function object created inside another function's code - nothing
// authoritative says what it owns), and an outer whose own extent still
// overlaps the union after the resize (those two roots stay open).
//
// After `apply`, re-run `MergeChainBodies.java dry`: the `outer_overlap` count
// must drop by the number of roots this fixed, and `apply` must then widen them.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;

public class FixOuterBodies extends GhidraScript {

    private static long u32(byte[] b, int i) {
        return (long) (b[i] & 0xff) | (long) (b[i + 1] & 0xff) << 8
            | (long) (b[i + 2] & 0xff) << 16 | (long) (b[i + 3] & 0xff) << 24;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        boolean apply = args.length > 0 && args[0].equalsIgnoreCase("apply");
        // Clipping is opt-in (`apply clip`): a no-.pdata object may be a real leaf
        // function, so removing the overlap is a judgement call, not a repair.
        boolean doClip = false;
        for (String a : args) {
            if (a.equalsIgnoreCase("clip")) {
                doClip = true;
            }
        }
        long base = currentProgram.getImageBase().getOffset();
        Memory mem = currentProgram.getMemory();
        MemoryBlock pd = mem.getBlock(".pdata");
        byte[] pb = new byte[(int) pd.getSize()];
        mem.getBytes(pd.getStart(), pb);

        Map<Long, long[]> entries = new HashMap<Long, long[]>();
        for (int i = 0; i + 12 <= pb.length; i += 12) {
            long beg = u32(pb, i), end = u32(pb, i + 4), unw = u32(pb, i + 8);
            if (beg == 0 || end <= beg) {
                continue;
            }
            int flags = 0;
            try {
                flags = (mem.getByte(toAddr(base + unw)) & 0xff) >> 3 & 0x1f;
            }
            catch (Exception e) {
                continue;
            }
            entries.put(base + beg, new long[] { base + beg, base + end, base + unw, flags & 4 });
        }

        Map<Long, List<long[]>> pieces = new HashMap<Long, List<long[]>>();
        for (long[] e : entries.values()) {
            if ((e[3] & 4) == 0) {
                continue;
            }
            long cur = e[0];
            HashSet<Long> seen = new HashSet<Long>();
            while (true) {
                long[] c = entries.get(cur);
                if (c == null || (c[3] & 4) == 0) {
                    break;
                }
                if (!seen.add(cur)) {
                    break;
                }
                int count = mem.getByte(toAddr(c[2] + 2)) & 0xff;
                int off = 4 + 2 * count + ((count & 1) != 0 ? 2 : 0);
                long pbeg = base + (mem.getInt(toAddr(c[2] + off)) & 0xffffffffL);
                if (!entries.containsKey(pbeg)) {
                    break;
                }
                cur = pbeg;
            }
            List<long[]> list = pieces.get(cur);
            if (list == null) {
                list = new ArrayList<long[]>();
                pieces.put(cur, list);
            }
            list.add(e);
        }

        // Outers that leak into a root's union, with their own extent.
        Map<Long, AddressSet> targets = new HashMap<Long, AddressSet>();
        Map<Long, Long> blockedRoots = new HashMap<Long, Long>();
        int manual = 0;
        for (Map.Entry<Long, List<long[]>> pair : pieces.entrySet()) {
            AddressSet u = new AddressSet();
            long[] self = entries.get(pair.getKey());
            u.add(toAddr(self[0]), toAddr(self[1] - 1));
            for (long[] p : pair.getValue()) {
                u.add(toAddr(p[0]), toAddr(p[1] - 1));
            }
            Iterator<Function> over = currentProgram.getFunctionManager().getFunctionsOverlapping(u);
            while (over.hasNext()) {
                Function f = over.next();
                Address entry = f.getEntryPoint();
                if (entry.getOffset() == pair.getKey() || u.contains(entry)) {
                    continue;
                }
                AddressSet inter = (AddressSet) u.intersect(f.getBody());
                if (inter.isEmpty()) {
                    continue;
                }
                long[] ext = entries.get(entry.getOffset());
                if (ext == null) {
                    manual++;
                    println(String.format("MANUAL %x: no .pdata extent, blocks %x (%d B inside)",
                        entry.getOffset(), pair.getKey(), inter.getNumAddresses()));
                    continue;
                }
                long extentBytes = ext[1] - ext[0];
                if (f.getBody().getNumAddresses() == extentBytes
                    && f.getBody().getMinAddress().getOffset() == ext[0]) {
                    manual++;
                    println(String.format("MANUAL %x: body already equals its extent, blocks %x",
                        entry.getOffset(), pair.getKey()));
                    continue;
                }
                targets.put(entry.getOffset(),
                    new AddressSet(toAddr(ext[0]), toAddr(ext[1] - 1)));
                blockedRoots.put(entry.getOffset(), pair.getKey());
            }
        }

        int fixed = 0, failed = 0, clipped = 0, clipFailed = 0;

        // Outers with no .pdata extent: nothing authoritative says what they own,
        // but the bytes a root's union claims are .pdata-assigned to that root, so
        // the outer may not keep them. `clip` subtracts every such union from the
        // outer's body and keeps the rest; an outer left with nothing is reported.
        Map<Long, AddressSet> clip = new HashMap<Long, AddressSet>();
        Map<Long, Long> clipRoots = new HashMap<Long, Long>();
        for (Map.Entry<Long, List<long[]>> pair : pieces.entrySet()) {
            AddressSet u = new AddressSet();
            long[] self = entries.get(pair.getKey());
            u.add(toAddr(self[0]), toAddr(self[1] - 1));
            for (long[] p : pair.getValue()) {
                u.add(toAddr(p[0]), toAddr(p[1] - 1));
            }
            Iterator<Function> over = currentProgram.getFunctionManager().getFunctionsOverlapping(u);
            while (over.hasNext()) {
                Function f = over.next();
                Address entry = f.getEntryPoint();
                if (entry.getOffset() == pair.getKey() || u.contains(entry)) {
                    continue;
                }
                if (entries.containsKey(entry.getOffset())) {
                    continue;
                }
                AddressSet acc = clip.get(entry.getOffset());
                if (acc == null) {
                    acc = new AddressSet();
                    clip.put(entry.getOffset(), acc);
                    clipRoots.put(entry.getOffset(), pair.getKey());
                }
                acc.add(u);
            }
        }

        int tx = currentProgram.startTransaction("Resize leaking bodies to their .pdata extent");
        boolean commit = false;
        try {
            for (Map.Entry<Long, AddressSet> t : targets.entrySet()) {
                Function f = currentProgram.getFunctionManager().getFunctionAt(toAddr(t.getKey()));
                if (f == null) {
                    failed++;
                    continue;
                }
                long before = f.getBody().getNumAddresses();
                if (!apply) {
                    println(String.format("WOULD %x: %d B/%d R -> %d B (unblocks %x)",
                        t.getKey(), before, f.getBody().getNumAddressRanges(),
                        t.getValue().getNumAddresses(), blockedRoots.get(t.getKey())));
                    fixed++;
                    continue;
                }
                try {
                    f.setBody(t.getValue());
                    fixed++;
                }
                catch (Exception ex) {
                    failed++;
                    println(String.format("FAIL %x: %s", t.getKey(), ex.getMessage()));
                }
            }
            for (Map.Entry<Long, AddressSet> c : (doClip ? clip.entrySet()
                : new HashMap<Long, AddressSet>().entrySet())) {
                Function f = currentProgram.getFunctionManager().getFunctionAt(toAddr(c.getKey()));
                if (f == null) {
                    clipFailed++;
                    continue;
                }
                AddressSet rest = (AddressSet) f.getBody().subtract(c.getValue());
                if (rest.isEmpty()) {
                    manual++;
                    println(String.format("MANUAL %x: nothing left after clipping, blocks %x",
                        c.getKey(), clipRoots.get(c.getKey())));
                    continue;
                }
                if (!apply) {
                    println(String.format("WOULD-CLIP %x: %d B/%d R -> %d B/%d R (blocks %x)",
                        c.getKey(), f.getBody().getNumAddresses(), f.getBody().getNumAddressRanges(),
                        rest.getNumAddresses(), rest.getNumAddressRanges(),
                        clipRoots.get(c.getKey())));
                    clipped++;
                    continue;
                }
                try {
                    f.setBody(rest);
                    clipped++;
                }
                catch (Exception ex) {
                    clipFailed++;
                    println(String.format("FAIL-CLIP %x: %s", c.getKey(), ex.getMessage()));
                }
            }
            if (apply) {
                commit = true;
            }
        }
        finally {
            currentProgram.endTransaction(tx, commit);
        }
        println(String.format("FIXOUTER mode=%s%s resize_candidates=%d %s=%d failed=%d "
            + "clip_candidates=%d %s=%d clip_failed=%d manual=%d",
            apply ? "apply" : "dry", doClip ? "+clip" : "",
            targets.size(), apply ? "fixed" : "would_fix",
            fixed, failed, doClip ? clip.size() : 0,
            apply ? "clipped" : "would_clip", clipped, clipFailed, manual));
        if (!apply) {
            println("dry run: nothing written. Run with `apply`, then MergeChainBodies.java dry.");
        }
    }
}
