// Read-only: list every function-map defect in the current image, so a repair
// pass can work from exact addresses instead of re-deriving them.
//
// Prints, one section per defect:
//   MISSING_REAL        real .pdata starts with no function object
//   AT_CHAIN            function objects sitting on a chained fragment (artifacts)
//   INSIDE              function entries inside another real extent
//   OVER                bodies longer than their .pdata extent (bytes beyond)
//   SHORT               bodies shorter than their extent
//   CHAIN_ROOTS_NAMED   named functions whose extent has chained continuations
//
// Usage: analyzeHeadless <proj> <projname> -process Overwatch.exe -noanalysis
//          -postScript PdataRepairList.java
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.SourceType;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

public class PdataRepairList extends GhidraScript {
    private long u32(byte[] b, int i) {
        return (b[i] & 0xffL) | ((b[i + 1] & 0xffL) << 8) | ((b[i + 2] & 0xffL) << 16)
            | ((b[i + 3] & 0xffL) << 24);
    }

    @Override
    public void run() throws Exception {
        Memory mem = currentProgram.getMemory();
        Listing listing = currentProgram.getListing();
        FunctionManager fm = currentProgram.getFunctionManager();
        long base = currentProgram.getImageBase().getOffset();
        byte[] pb = new byte[(int) mem.getBlock(".pdata").getSize()];
        mem.getBytes(mem.getBlock(".pdata").getStart(), pb);

        HashMap<Long, long[]> entries = new HashMap<Long, long[]>();
        TreeMap<Long, long[]> realByBegin = new TreeMap<Long, long[]>();
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
            }
            long[] ent = { base + beg, base + end, base + unw, flags & 4 };
            entries.put(ent[0], ent);
            if ((flags & 4) == 0) {
                realByBegin.put(ent[0], ent);
            }
        }

        List<String> missing = new ArrayList<String>();
        for (long[] e : realByBegin.values()) {
            if (fm.getFunctionAt(toAddr(e[0])) == null) {
                missing.add(String.format("%x", e[0]));
            }
        }
        println("MISSING_REAL count=" + missing.size() + " " + missing);

        // resolve every chained fragment to its root once
        HashMap<Long, long[]> rootAgg = new HashMap<Long, long[]>();
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
                int count = 0;
                try {
                    count = mem.getByte(toAddr(c[2] + 2)) & 0xff;
                }
                catch (Exception ex) {
                }
                int off = 4 + 2 * count + ((count & 1) != 0 ? 2 : 0);
                long pbeg = 0;
                try {
                    pbeg = base + (mem.getInt(toAddr(c[2] + off)) & 0xffffffffL);
                }
                catch (Exception ex) {
                    break;
                }
                if (entries.get(pbeg) == null) {
                    break;
                }
                cur = pbeg;
            }
            long[] agg = rootAgg.get(cur);
            if (agg == null) {
                agg = new long[] { 0, 0 };
                rootAgg.put(cur, agg);
            }
            agg[0]++;
            agg[1] += e[1] - e[0];
        }

        List<String> atChain = new ArrayList<String>();
        List<String> inside = new ArrayList<String>();
        List<String> over = new ArrayList<String>();
        List<String> shortB = new ArrayList<String>();
        List<String> namedChains = new ArrayList<String>();
        int shortRet = 0;
        for (Function f : fm.getFunctions(true)) {
            long ep = f.getEntryPoint().getOffset();
            long[] e = entries.get(ep);
            boolean isNamed = f.getSymbol().getSource() != SourceType.DEFAULT;
            if (e != null && (e[3] & 4) != 0) {
                atChain.add(String.format("%x/%s", ep, f.getName()));
                continue;
            }
            if (e == null) {
                continue;
            }
            long extLen = e[1] - e[0];
            long bodyLen = f.getBody().getNumAddresses();
            if (bodyLen > extLen) {
                over.add(String.format("%x/%s/ext=%x/over=%d/ranges=%d", ep, f.getName(), extLen,
                    bodyLen - extLen, f.getBody().getNumAddressRanges()));
            }
            else if (bodyLen < extLen) {
                Instruction last = listing.getInstructionContaining(toAddr(e[1] - 1));
                boolean ret = last != null && last.getMnemonicString().startsWith("RET");
                if (ret) {
                    shortRet++;
                }
                shortB.add(String.format("%x/ext=%x/body=%d/endret=%b", ep, extLen, bodyLen, ret));
            }
            Map.Entry<Long, long[]> fl = realByBegin.floorEntry(ep);
            if (fl != null && fl.getKey() != ep && ep < fl.getValue()[1]) {
                Function c = fm.getFunctionAt(toAddr(fl.getKey()));
                inside.add(String.format("%x/%s/inside=%x/%s", ep, f.getName(), fl.getKey(),
                    c == null ? "?" : c.getName()));
            }
            long[] agg = rootAgg.get(ep);
            if (isNamed && agg != null) {
                namedChains.add(String.format("%x/%s/pieces=%d/bytes=%d", ep, f.getName(), agg[0],
                    agg[1]));
            }
        }
        println("AT_CHAIN count=" + atChain.size() + " " + atChain);
        println("INSIDE count=" + inside.size() + " " + inside);
        println("OVER count=" + over.size() + " " + over);
        println("SHORT count=" + shortB.size() + " ending_in_ret=" + shortRet + " " + shortB);
        println("CHAIN_ROOTS_NAMED count=" + namedChains.size());
        println("CHAIN_ROOTS_NAMED_LIST " + namedChains);
        println("REPAIRLIST DONE");
    }
}
