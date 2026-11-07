// Create functions for referenced code targets that Ghidra's analysis skipped.
//
//   RecoverPointerFunctions.java <mode> [limit]
//
// mode: int3   - only targets preceded by INT3 padding (conservative)
//       padded - also targets preceded by RET (0xC3) or zero padding
//       all    - every referenced target still outside a function
//       dry    - report only: what would be created, nothing written
//
// limit: stop after this many creations (0 or omitted = all).
//
// Idempotent: targets already inside a function are skipped.
//
// GUARD - why this pass is a no-op on a complete map, and must stay one.
//
// The pointer scan finds every 8-byte slot in `.rdata`/`.data` that holds a
// pointer into `.text`. Most such slots point into the *middle* of a function
// (EH handler data, funclet tables, vtables, jump tables), and every chained
// `.pdata` fragment start is one of them. Creating a function at those targets
// is what produced the junk function map this project had to clean up: the
// count went past 200,000 and every name lookup afterwards saw invented
// callers. So a target is created only when the client's own table says it is
// a function start:
//
//   - a target inside any `.pdata` extent (real or chained) is skipped;
//   - a target at a `UNW_FLAG_CHAININFO` start is skipped;
//   - a target that is not a real `.pdata` start is skipped.
//
// On the current image that leaves nothing to create (every real start already
// has a function), which is the correct answer, not a failure. The legit use is
// a fresh import that ran DisassemblePdataFunctions.java with `skipped > 0`
// crossings. A `.pdata`-less target has no readable extent, so a function there
// would be an inferred body, not a read - see
// docs/ghidra-image-audit.md, section 8.8.
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.cmd.function.CreateFunctionCmd;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;

public class RecoverPointerFunctions extends GhidraScript {
    @Override
    public void run() throws Exception {
        long base = 0x140000000L;
        long tlo = 0x1000;
        Memory mem0 = currentProgram.getMemory();
        long thi = mem0.getBlock(".text").getEnd().getOffset() - 0x140000000L;
        String[] args = getScriptArgs();
        // The bridge hands args over as a list; accept "[dry]" and "dry" alike.
        StringBuilder letters = new StringBuilder();
        if (args.length > 0) for (char c : args[0].toCharArray()) if (Character.isLetter(c)) letters.append(c);
        String mode = letters.length() == 0 ? "all" : letters.toString();
        int limit = 0;
        if (args.length > 1) for (char c : args[1].toCharArray()) if (Character.isDigit(c)) limit = limit * 10 + (c - '0');
        Memory mem = currentProgram.getMemory();
        FunctionManager fm = currentProgram.getFunctionManager();

        // The client's own table: which extents exist, which are chain
        // continuations, and which starts are real functions.
        java.util.TreeMap<Long, Long> extentAt = new java.util.TreeMap<Long, Long>();
        java.util.HashSet<Long> chainedStart = new java.util.HashSet<Long>();
        java.util.HashSet<Long> realStart = new java.util.HashSet<Long>();
        MemoryBlock pd = mem.getBlock(".pdata");
        long npd = 0;
        for (long off = 0; off + 12 <= pd.getSize(); off += 12) {
            long beg = mem.getInt(pd.getStart().add(off)) & 0xFFFFFFFFL;
            long end = mem.getInt(pd.getStart().add(off + 4)) & 0xFFFFFFFFL;
            long unwind = mem.getInt(pd.getStart().add(off + 8)) & 0xFFFFFFFFL;
            if (beg == 0 || end <= beg || end >= thi) continue;
            int flags = 0;
            try { flags = mem.getByte(toAddr(base + unwind)) & 0xFF; } catch (Exception e) { continue; }
            boolean chain = ((flags >> 3) & 0x1F & 0x4) != 0;
            if (chain) chainedStart.add(beg); else realStart.add(beg);
            extentAt.put(beg, end);
            npd++;
        }
        println("PDATA extents=" + npd + " real=" + realStart.size() + " chained=" + chainedStart.size());

        java.util.HashSet<Long> tgts = new java.util.HashSet<Long>();
        for (MemoryBlock b : mem.getBlocks()) {
            String n = b.getName();
            if (!n.equals(".rdata") && !n.equals(".data")) continue;
            for (Address a = b.getStart(); a.add(7).compareTo(b.getEnd()) <= 0; a = a.add(8)) {
                long v = mem.getLong(a);
                if (v >= base + tlo && v < base + thi) tgts.add(v - base);
            }
        }
        int created = 0, failed = 0, considered = 0, wouldCreate = 0;
        boolean dry = mode.equals("dry");
        int skipInExtent = 0, skipNotStart = 0;
        long largest = 0;
        Address largestAt = null;
        for (long r : tgts) {
            if (limit > 0 && considered >= limit) break;
            if (!realStart.contains(r)) {
                if (chainedStart.contains(r)) skipInExtent++;
                else if (extentAt.floorKey(r) != null && r < extentAt.get(extentAt.floorKey(r))) skipInExtent++;
                else skipNotStart++;
                continue;
            }
            Address ad = toAddr(base + r);
            if (fm.getFunctionAt(ad) != null) continue;
            if (getFunctionContaining(ad) != null) continue;
            int prev = -1;
            try { prev = mem.getByte(ad.subtract(1)) & 0xFF; } catch (Exception e) { }
            if (mode.equals("int3") && prev != 0xCC) continue;
            if (mode.equals("padded") && !(prev == 0xCC || prev == 0xC3 || prev == 0x00)) continue;
            considered++;
            if (dry) {
                Address d = toAddr(base + r);
                if (fm.getFunctionAt(d) == null && getFunctionContaining(d) == null) {
                    wouldCreate++;
                    if (wouldCreate <= 20) println("WOULD CREATE " + d);
                }
                continue;
            }
            try {
                new DisassembleCommand(ad, null, true).applyTo(currentProgram, monitor);
                boolean ok = new CreateFunctionCmd(ad).applyTo(currentProgram, monitor);
                Function f = fm.getFunctionAt(ad);
                if (ok && f != null) {
                    created++;
                    long sz = f.getBody().getNumAddresses();
                    if (sz > largest) { largest = sz; largestAt = ad; }
                } else {
                    failed++;
                }
            } catch (Exception e) {
                failed++;
            }
            if ((considered % 1000) == 0) println("progress considered=" + considered + " created=" + created);
        }
        println("DONE mode=" + mode + " targets=" + tgts.size() + " considered=" + considered
            + " created=" + created + " failed=" + failed
            + " skipped_in_extent=" + skipInExtent + " skipped_not_pdata_start=" + skipNotStart
            + " would_create=" + wouldCreate
            + " largest=" + largest + " at " + largestAt);
    }
}
