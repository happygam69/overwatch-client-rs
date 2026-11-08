// ChainConflicts.java - who else owns bytes inside a chained root's union?
//
// Usage: ChainConflicts.java [out.tsv]     (default C:/Temp/owaudit/conflicts.tsv)
//
// `MergeChainBodies.java` refuses to widen a root when a function whose ENTRY is
// outside the union still has a body reaching in - widening would delete that
// function's extent. This script lists every such pair with the numbers needed
// to decide the fix:
//
//   root, rootUnionBeg/End   the root and the span it should own
//   outer, outerBody         the intruding function: entry, body bytes, ranges
//   outerExtent              its own .pdata real extent, empty when it is not a
//                            real .pdata start
//   outerUnionBeg/End        the union its own CHAININFO resolves to, empty when
//                            it has no chained pieces
//   overlap                  bytes of the root's union that sit inside the
//                            outer's body
//
// Rule of thumb: when `outerBody` exceeds `outerUnion`, the outer is
// over-extended (the audit's 26-body class) and the fix is to set its body back
// to its own union; the root then widens normally on the next merge pass.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;

import java.io.BufferedWriter;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;

public class ChainConflicts extends GhidraScript {

    private static long u32(byte[] b, int i) {
        return (long) (b[i] & 0xff) | (long) (b[i + 1] & 0xff) << 8
            | (long) (b[i + 2] & 0xff) << 16 | (long) (b[i + 3] & 0xff) << 24;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String out = args.length > 0 ? args[0] : "C:/Temp/owaudit/conflicts.tsv";
        long base = currentProgram.getImageBase().getOffset();
        Memory mem = currentProgram.getMemory();
        MemoryBlock pd = mem.getBlock(".pdata");
        byte[] pb = new byte[(int) pd.getSize()];
        mem.getBytes(pd.getStart(), pb);

        Map<Long, long[]> entries = new HashMap<Long, long[]>(); // beg -> {beg,end,unw,chained}
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

        // Resolve every chained entry to its transitive root.
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

        // Every root's union, so an intruder's own union can be reported too.
        Map<Long, AddressSet> unions = new HashMap<Long, AddressSet>();
        for (Map.Entry<Long, List<long[]>> pair : pieces.entrySet()) {
            AddressSet u = new AddressSet();
            for (long[] p : pair.getValue()) {
                u.add(toAddr(p[0]), toAddr(p[1] - 1));
            }
            long[] self = entries.get(pair.getKey());
            if (self != null) {
                u.add(toAddr(self[0]), toAddr(self[1] - 1));
            }
            unions.put(pair.getKey(), u);
        }

        int conflicts = 0, rootsAffected = 0;
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(out)))) {
            w.println("root\tunionBeg\tunionEnd\touter\touterBody\touterRanges\touterExtent\touterUnionBeg\touterUnionEnd\toverlap");
            for (Map.Entry<Long, List<long[]>> pair : pieces.entrySet()) {
                AddressSet u = unions.get(pair.getKey());
                boolean any = false;
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
                    AddressSet ou = unions.get(entry.getOffset());
                    w.println(String.format("%x\t%x\t%x\t%x\t%d\t%d\t%s\t%s\t%s\t%d",
                        pair.getKey(), u.getMinAddress().getOffset(), u.getMaxAddress().getOffset() + 1,
                        entry.getOffset(), f.getBody().getNumAddresses(),
                        f.getBody().getNumAddressRanges(),
                        ext == null ? "" : String.format("%x..%x", ext[0], ext[1]),
                        ou == null ? "" : String.format("%x", ou.getMinAddress().getOffset()),
                        ou == null ? "" : String.format("%x", ou.getMaxAddress().getOffset() + 1),
                        inter.getNumAddresses()));
                    conflicts++;
                    any = true;
                }
                if (any) {
                    rootsAffected++;
                }
            }
        }
        println(String.format("CONFLICTS roots=%d conflicts=%d -> %s", rootsAffected, conflicts, out));
    }
}
