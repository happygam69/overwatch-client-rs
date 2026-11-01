// Read-only census of the function map against the executable's own .pdata.
//
// Counts the .pdata entries, the real functions (UNW_FLAG_CHAININFO clear),
// the chained fragments, and where the program's function symbols sit:
//
//   real start       - a function at a real .pdata entry (the authority)
//   chained start    - a function at a chained-fragment entry (not callable)
//   inside real      - a function starting inside another real function
//   non-pdata        - a leaf/local function with no unwind entry
//
// It also counts named functions, user prototypes and named parameters, so a
// clean import and the live program can be compared by numbers, not by eye.
//
// Usage: analyzeHeadless ... -postScript FunctionCensus.java
//        run_ghidra_script FunctionCensus.java
//
// This script changes nothing.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.listing.Parameter;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.SourceType;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.Iterator;

public class FunctionCensus extends GhidraScript {
    @Override
    public void run() throws Exception {
        Memory mem = currentProgram.getMemory();
        long base = currentProgram.getImageBase().getOffset();
        MemoryBlock pd = mem.getBlock(".pdata");
        if (pd == null) {
            println("CENSUS no .pdata block");
            return;
        }
        long size = Math.min(pd.getSize(), Integer.MAX_VALUE);
        byte[] b = new byte[(int) size];
        int got = mem.getBytes(pd.getStart(), b);
        int entries = 0, real = 0, chained = 0;
        HashSet<Long> realBegins = new HashSet<Long>();
        ArrayList<long[]> realRanges = new ArrayList<long[]>();
        for (int i = 0; i + 12 <= got; i += 12) {
            long beg = (b[i] & 0xffL) | ((b[i + 1] & 0xffL) << 8)
                | ((b[i + 2] & 0xffL) << 16) | ((b[i + 3] & 0xffL) << 24);
            long end = (b[i + 4] & 0xffL) | ((b[i + 5] & 0xffL) << 8)
                | ((b[i + 6] & 0xffL) << 16) | ((b[i + 7] & 0xffL) << 24);
            long unw = (b[i + 8] & 0xffL) | ((b[i + 9] & 0xffL) << 8)
                | ((b[i + 10] & 0xffL) << 16) | ((b[i + 11] & 0xffL) << 24);
            if (beg == 0 || end <= beg) {
                continue;
            }
            entries++;
            int flags = 0;
            try {
                flags = (mem.getByte(toAddr(base + unw)) & 0xff) >> 3 & 0x1f;
            }
            catch (Exception e) {
                // No unwind bytes: treat the entry as a real function.
            }
            if ((flags & 4) != 0) {
                chained++;
            }
            else {
                real++;
                realBegins.add(base + beg);
                realRanges.add(new long[] { base + beg, base + end });
            }
        }
        realRanges.sort((x, y) -> Long.compare(x[0], y[0]));
        long[] starts = new long[realRanges.size()];
        for (int i = 0; i < starts.length; i++) {
            starts[i] = realRanges.get(i)[0];
        }

        FunctionManager fm = currentProgram.getFunctionManager();
        int total = 0, atReal = 0, atChained = 0, insideReal = 0, nonPdata = 0;
        int named = 0, userProto = 0, namedParams = 0, empty = 0;
        long instructions = 0;
        for (Function f : fm.getFunctions(true)) {
            total++;
            int count = 0;
            InstructionIterator ii = currentProgram.getListing().getInstructions(f.getBody(), true);
            while (ii.hasNext()) {
                ii.next();
                count++;
            }
            if (count == 0) {
                empty++;
            }
            instructions += count;
            long e = f.getEntryPoint().getOffset();
            if (realBegins.contains(e)) {
                atReal++;
            }
            else {
                nonPdata++;
                int idx = Arrays.binarySearch(starts, e);
                if (idx < 0) {
                    idx = -idx - 2;
                }
                if (idx >= 0 && realRanges.get(idx)[0] <= e && e < realRanges.get(idx)[1]) {
                    insideReal++;
                }
            }
            if (f.getSymbol().getSource() != SourceType.DEFAULT) {
                named++;
            }
            if (f.getSignatureSource() == SourceType.USER_DEFINED) {
                userProto++;
            }
            for (Parameter p : f.getParameters()) {
                if (p.getName() != null && !p.getName().startsWith("param_")) {
                    namedParams++;
                    break;
                }
            }
        }
        println("CENSUS entries=" + entries + " real=" + real + " chained=" + chained
            + " totalFns=" + total + " atReal=" + atReal + " atChained=" + atChained
            + " insideReal=" + insideReal + " nonPdata=" + nonPdata
            + " named=" + named + " userProto=" + userProto + " namedParams=" + namedParams
            + " emptyFns=" + empty + " instructions=" + instructions);
    }
}
