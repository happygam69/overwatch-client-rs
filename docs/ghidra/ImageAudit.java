// Read-only image audit: one script that re-measures every number in
// docs/ghidra-image-audit.md, so the audit can be re-run after a repair and
// diffed. Nothing is changed: no instruction, no function, no symbol.
//
// Usage: analyzeHeadless <proj> <projname> -process Overwatch.exe -noanalysis
//          -postScript ImageAudit.java
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressRange;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.CommentType;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.ReferenceIterator;
import ghidra.program.model.symbol.SourceType;

import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;

public class ImageAudit extends GhidraScript {
    private static final String[] BLOCKS =
        { "Headers", ".text", ".rdata", ".data", ".pdata", "_RDATA", ".rodata", ".rsrc", ".reloc" };

    private long u32(byte[] b, int i) {
        return (b[i] & 0xffL) | ((b[i + 1] & 0xffL) << 8) | ((b[i + 2] & 0xffL) << 16)
            | ((b[i + 3] & 0xffL) << 24);
    }

    private String sha(byte[] b) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        StringBuilder s = new StringBuilder();
        for (byte x : md.digest(b)) {
            s.append(String.format("%02x", x & 0xff));
        }
        return s.toString();
    }

    @Override
    public void run() throws Exception {
        Memory mem = currentProgram.getMemory();
        Listing listing = currentProgram.getListing();
        FunctionManager fm = currentProgram.getFunctionManager();
        long base = currentProgram.getImageBase().getOffset();

        // ---- 1. image bytes
        long total = 0;
        for (String name : BLOCKS) {
            MemoryBlock blk = mem.getBlock(name);
            if (blk == null) {
                println("IMAGE " + name + " MISSING");
                continue;
            }
            byte[] b = new byte[(int) blk.getSize()];
            mem.getBytes(blk.getStart(), b);
            total += b.length;
            println(String.format("IMAGE %s start=%x size=%d sha256=%s", name,
                blk.getStart().getOffset(), b.length, sha(b)));
        }
        println("IMAGE total_bytes=" + total);

        // ---- 2. .pdata census and .text coverage
        MemoryBlock pd = mem.getBlock(".pdata");
        MemoryBlock text = mem.getBlock(".text");
        byte[] pb = new byte[(int) pd.getSize()];
        mem.getBytes(pd.getStart(), pb);
        HashMap<Long, long[]> entries = new HashMap<Long, long[]>();
        AddressSet realSet = new AddressSet();
        AddressSet chainSet = new AddressSet();
        int real = 0, chained = 0, badUnwind = 0;
        long realBytes = 0, chainBytes = 0;
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
                badUnwind++;
            }
            entries.put(base + beg, new long[] { base + beg, base + end, base + unw, flags & 4 });
            if ((flags & 4) != 0) {
                chained++;
                chainBytes += end - beg;
                chainSet.add(toAddr(base + beg), toAddr(base + end - 1));
            }
            else {
                real++;
                realBytes += end - beg;
                realSet.add(toAddr(base + beg), toAddr(base + end - 1));
            }
        }
        AddressSet textSet = new AddressSet(text.getStart(), text.getEnd());
        long textBytes = textSet.getNumAddresses();
        long gapBytes = 0, bigGapBytes = 0;
        int gapRuns = 0, bigGaps = 0;
        for (AddressRange r : textSet.subtract(realSet).subtract(chainSet)) {
            gapRuns++;
            gapBytes += r.getLength();
            if (r.getLength() >= 256) {
                bigGaps++;
                bigGapBytes += r.getLength();
            }
        }
        println(String.format(
            "PDATA entries=%d real=%d chained=%d bad_unwind=%d real_bytes=%d chain_bytes=%d text_bytes=%d covered=%.2f%% gaps: runs=%d bytes=%d big=%d big_bytes=%d",
            entries.size(), real, chained, badUnwind, realBytes, chainBytes, textBytes,
            100.0 * (realBytes + chainBytes) / textBytes, gapRuns, gapBytes, bigGaps, bigGapBytes));

        // ---- 3. resolve chains to roots
        HashMap<Long, List<long[]>> pieces = new HashMap<Long, List<long[]>>();
        int unresolvable = 0;
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
                    unresolvable++;
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
                    unresolvable++;
                    break;
                }
                if (!entries.containsKey(pbeg)) {
                    unresolvable++;
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
        println(String.format("CHAINS roots=%d pieces=%d unresolvable=%d", pieces.size(), chained,
            unresolvable));

        // ---- 4. function map
        AddressSet bodySet = new AddressSet();
        int fns = 0, atReal = 0, atChain = 0, noPdata = 0, insideReal = 0, insideChain = 0;
        int empty = 0, named = 0, userProto = 0, namedParams = 0, thunks = 0;
        int shortBody = 0, shortRet = 0, exact = 0, over = 0, frag = 0;
        long overBytes = 0;
        List<String> chainStarts = new ArrayList<String>();
        for (Function f : fm.getFunctions(true)) {
            fns++;
            bodySet.add(f.getBody());
            long ep = f.getEntryPoint().getOffset();
            long[] e = entries.get(ep);
            if (f.getBody().getNumAddresses() == 0) {
                empty++;
            }
            if (f.getSymbol().getSource() != SourceType.DEFAULT) {
                named++;
            }
            if (f.getSignatureSource() == SourceType.USER_DEFINED) {
                userProto++;
            }
            for (int i = 0; i < f.getParameterCount(); i++) {
                String pn = f.getParameter(i).getName();
                if (pn != null && !pn.isEmpty() && !pn.equals("param_" + (i + 1))) {
                    namedParams++;
                }
            }
            if (f.isThunk()) {
                thunks++;
            }
            if (e == null) {
                noPdata++;
                continue;
            }
            if ((e[3] & 4) != 0) {
                atChain++;
                if (chainStarts.size() < 400) {
                    chainStarts.add(String.format("%x/%s", ep, f.getName()));
                }
                continue;
            }
            atReal++;
            long extLen = e[1] - e[0];
            long bodyLen = f.getBody().getNumAddresses();
            if (bodyLen == extLen) {
                exact++;
            }
            else if (bodyLen < extLen) {
                shortBody++;
                Instruction last = listing.getInstructionContaining(toAddr(e[1] - 1));
                if (last != null && last.getMnemonicString().startsWith("RET")) {
                    shortRet++;
                }
            }
            else {
                over++;
                overBytes += bodyLen - extLen;
            }
            if (f.getBody().getNumAddressRanges() > 1) {
                frag++;
            }
        }
        List<String> missing = new ArrayList<String>();
        for (long[] e : entries.values()) {
            if ((e[3] & 4) == 0 && fm.getFunctionAt(toAddr(e[0])) == null) {
                missing.add(String.format("%x", e[0]));
            }
        }
        for (Function f : fm.getFunctions(true)) {
            Function c = fm.getFunctionContaining(f.getEntryPoint());
            if (c == null || c == f) {
                continue;
            }
            long[] e = entries.get(f.getEntryPoint().getOffset());
            if (e != null && (e[3] & 4) != 0) {
                insideChain++;
            }
            else {
                insideReal++;
            }
        }
        println(String.format(
            "FUNCS total=%d at_real_start=%d at_chain_start=%d no_pdata=%d inside_other_real=%d inside_other_chain=%d real_starts_missing=%d empty=%d named=%d user_proto=%d named_params=%d thunks=%d bodies: exact=%d short=%d short_ending_in_ret=%d over=%d over_bytes=%d fragmented=%d",
            fns, atReal, atChain, noPdata, insideReal, insideChain, missing.size(), empty, named,
            userProto, namedParams, thunks, exact, shortBody, shortRet, over, overBytes, frag));
        println("FUNC_LIST at_chain_start=" + chainStarts);
        println("FUNC_LIST real_starts_missing=" + missing);

        // ---- 5. instruction ownership and defined/undefined bytes in .text
        long insTotal = 0, insBytesTotal = 0, insBody = 0, insBodyBytes = 0;
        long insBodyReal = 0, insBodyChain = 0, insBodyGap = 0;
        long insBodyBytesReal = 0, insBodyBytesChain = 0, insBodyBytesGap = 0;
        long insBytesReal = 0, insBytesChain = 0, insBytesOutside = 0;
        for (Instruction ins : listing.getInstructions(textSet, true)) {
            int len = ins.getLength();
            insTotal++;
            insBytesTotal += len;
            Address a = ins.getAddress();
            boolean inReal = realSet.contains(a), inChain = chainSet.contains(a);
            if (inReal) {
                insBytesReal += len;
            }
            else if (inChain) {
                insBytesChain += len;
            }
            else {
                insBytesOutside += len;
            }
            if (bodySet.contains(a)) {
                insBody++;
                insBodyBytes += len;
                if (inReal) {
                    insBodyReal++;
                    insBodyBytesReal += len;
                }
                else if (inChain) {
                    insBodyChain++;
                    insBodyBytesChain += len;
                }
                else {
                    insBodyGap++;
                    insBodyBytesGap += len;
                }
            }
        }
        long dataItems = 0, dataBytes = 0, dataBytesReal = 0, dataBytesChain = 0, dataBytesOutside = 0;
        for (Data d : listing.getDefinedData(textSet, true)) {
            dataItems++;
            long len = d.getLength();
            dataBytes += len;
            Address a = d.getAddress();
            if (realSet.contains(a)) {
                dataBytesReal += len;
            }
            else if (chainSet.contains(a)) {
                dataBytesChain += len;
            }
            else {
                dataBytesOutside += len;
            }
        }
        println(String.format(
            "OWNERSHIP instructions=%d ins_bytes=%d body_ins=%d body_bytes=%d body_in_real=%d body_in_chain=%d body_in_gap=%d orphan_base_real=%d orphan_base_chain=%d orphan_outside=%d | text_data_items=%d text_data_bytes=%d",
            insTotal, insBytesTotal, insBody, insBodyBytes, insBodyReal, insBodyChain, insBodyGap,
            insBytesReal - insBodyBytesReal, insBytesChain - insBodyBytesChain,
            insBytesOutside - insBodyBytesGap, dataItems, dataBytes));
        println(String.format(
            "UNDEFINED total=%d real=%d chain=%d outside=%d",
            textBytes - insBytesTotal - dataBytes, realBytes - insBytesReal - dataBytesReal,
            chainBytes - insBytesChain - dataBytesChain,
            gapBytes - insBytesOutside - dataBytesOutside));

        // ---- 6. chain impact
        int roots = 0, inFn = 0, flowIn = 0, namedRoots = 0, namedFlow = 0, chainRet = 0;
        for (Map.Entry<Long, List<long[]>> en : pieces.entrySet()) {
            Function f = fm.getFunctionAt(toAddr(en.getKey()));
            if (f == null) {
                continue;
            }
            roots++;
            boolean isNamed = f.getSymbol().getSource() != SourceType.DEFAULT;
            if (isNamed) {
                namedRoots++;
            }
            for (long[] p : en.getValue()) {
                if (f.getBody().intersects(toAddr(p[0]), toAddr(p[1] - 1))) {
                    inFn++;
                    break;
                }
            }
            for (long[] p : en.getValue()) {
                Instruction last = listing.getInstructionContaining(toAddr(p[1] - 1));
                if (last != null && last.getMnemonicString().startsWith("RET")) {
                    chainRet++;
                }
            }
            Instruction last = listing.getInstructionContaining(f.getBody().getMaxAddress());
            boolean flowed = false;
            if (last != null) {
                List<Address> outs = new ArrayList<Address>();
                if (last.getFallThrough() != null) {
                    outs.add(last.getFallThrough());
                }
                for (Address t : last.getFlows()) {
                    outs.add(t);
                }
                for (Address t : outs) {
                    if (f.getBody().contains(t)) {
                        continue;
                    }
                    for (long[] p : en.getValue()) {
                        if (t.getOffset() >= p[0] && t.getOffset() < p[1]) {
                            flowed = true;
                        }
                    }
                }
            }
            if (flowed) {
                flowIn++;
                if (isNamed) {
                    namedFlow++;
                }
            }
        }
        println(String.format(
            "CHAIN_IMPACT roots=%d in_body=%d body_flows_into_chain=%d named_roots=%d named_flowed=%d chain_pieces_ending_in_ret=%d",
            roots, inFn, flowIn, namedRoots, namedFlow, chainRet));

        // ---- 7. no-return damage
        long callSites = 0, callsWithCont = 0, affected = 0;
        HashSet<Long> callersSeen = new HashSet<Long>();
        int nor = 0;
        List<String> flags = new ArrayList<String>();
        for (Function g : fm.getFunctions(true)) {
            if (!g.hasNoReturn()) {
                continue;
            }
            nor++;
            int cont = 0;
            ReferenceIterator ri =
                currentProgram.getReferenceManager().getReferencesTo(g.getEntryPoint());
            while (ri.hasNext()) {
                Address from = ri.next().getFromAddress();
                Function caller = fm.getFunctionContaining(from);
                if (caller == null || caller == g) {
                    continue;
                }
                Instruction ins = listing.getInstructionContaining(from);
                if (ins != null && ins.getFlowType().isCall()) {
                    callSites++;
                    if (ins.getFallThrough() != null) {
                        callsWithCont++;
                        cont++;
                        if (callersSeen.add(caller.getEntryPoint().getOffset())) {
                            affected++;
                        }
                    }
                }
            }
            flags.add(String.format("%x/%s/cont=%d", g.getEntryPoint().getOffset(), g.getName(),
                cont));
        }
        println(String.format(
            "NORETURN flagged=%d call_sites=%d with_continuation=%d affected_callers=%d", nor,
            callSites, callsWithCont, affected));
        println("NORETURN_LIST " + flags);

        // ---- 8. data coverage per block
        for (String name : new String[] { ".rdata", ".data", ".rodata", "_RDATA", ".text" }) {
            MemoryBlock blk = mem.getBlock(name);
            if (blk == null) {
                continue;
            }
            AddressSet set = new AddressSet(blk.getStart(), blk.getEnd());
            long def = 0, items = 0, strs = 0;
            for (Data d : listing.getDefinedData(set, true)) {
                items++;
                def += d.getLength();
                if (d.getValue() instanceof String) {
                    strs++;
                }
            }
            println(String.format("DATA %s size=%d defined=%d pct=%.1f items=%d strings=%d", name,
                blk.getSize(), def, 100.0 * def / blk.getSize(), items, strs));
        }

        // ---- 9. documentation depth
        int plate = 0, pre = 0;
        for (Function f : fm.getFunctions(true)) {
            if (listing.getComment(CommentType.PLATE, f.getEntryPoint()) != null) {
                plate++;
            }
            if (listing.getComment(CommentType.PRE, f.getEntryPoint()) != null) {
                pre++;
            }
        }
        println(String.format("DOC plate=%d pre=%d", plate, pre));
        println("AUDIT DONE");
    }
}
