// Audit the function-recovery gap.
//
// .pdata has two kinds of entries:
//   real functions       (UNW_FLAG_CHAININFO clear)
//   chained fragments    (UNW_FLAG_CHAININFO set) - pieces of a larger
//                         function, not callable functions
//
// Reports both, the referenced code targets still outside functions, and the
// .text bytes covered by functions.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;

public class RecoveryAudit extends GhidraScript {
    @Override
    public void run() throws Exception {
        long base = 0x140000000L;
        long pdataVa = base + 0x450B000L;
        int n = 0x24D968 / 12;
        Memory mem = currentProgram.getMemory();
        FunctionManager fm = currentProgram.getFunctionManager();
        int realTotal = 0, realMissing = 0, chainTotal = 0, chainMissing = 0;
        for (int i = 0; i < n; i++) {
            long off = pdataVa + i * 12L;
            long bs = Integer.toUnsignedLong(mem.getInt(toAddr(off)));
            long be = Integer.toUnsignedLong(mem.getInt(toAddr(off + 4)));
            long u = Integer.toUnsignedLong(mem.getInt(toAddr(off + 8)));
            if (bs == 0 || be <= bs || bs < 0x1000 || be > 0x2F2B3D3) continue;
            int flags = 0;
            try { flags = (mem.getByte(toAddr(base + u)) & 0xFF) >> 3 & 0x1F; } catch (Exception e) {}
            boolean chained = (flags & 4) != 0;
            Address b = toAddr(base + bs);
            boolean present = fm.getFunctionAt(b) != null || getFunctionContaining(b) != null;
            if (chained) { chainTotal++; if (!present) chainMissing++; }
            else { realTotal++; if (!present) realMissing++; }
        }
        println("pdata real_functions=" + realTotal + " missing=" + realMissing);
        println("pdata chained_fragments=" + chainTotal + " without_function=" + chainMissing);

        java.util.HashSet<Long> tgts = new java.util.HashSet<Long>();
        for (MemoryBlock b : mem.getBlocks()) {
            String nm = b.getName();
            if (!nm.equals(".rdata") && !nm.equals(".data")) continue;
            for (Address a = b.getStart(); a.add(7).compareTo(b.getEnd()) <= 0; a = a.add(8)) {
                long v = mem.getLong(a);
                if (v >= base + 0x1000 && v < base + 0x2F2B3D3) tgts.add(v - base);
            }
        }
        int outside = 0;
        for (long r : tgts) {
            Address ad = toAddr(base + r);
            if (fm.getFunctionAt(ad) == null && getFunctionContaining(ad) == null) outside++;
        }
        println("pointer targets=" + tgts.size() + " outside=" + outside);

        MemoryBlock tb = mem.getBlock(".text");
        AddressSet all = new AddressSet(tb.getStart(), tb.getEnd());
        AddressSet funcs = new AddressSet();
        FunctionIterator fi = fm.getFunctions(true);
        while (fi.hasNext()) funcs.add(fi.next().getBody());
        AddressSet outsideSet = new AddressSet(all);
        outsideSet.delete(funcs);
        AddressSet defined = new AddressSet();
        InstructionIterator ii = currentProgram.getListing().getInstructions(all, true);
        while (ii.hasNext()) {
            Instruction ins = ii.next();
            defined.add(ins.getAddress(), ins.getAddress().add(ins.getLength() - 1));
        }
        AddressSetView undef = outsideSet.subtract(defined);
        println("text=" + all.getNumAddresses() + " funcs=" + funcs.getNumAddresses()
            + " outside=" + outsideSet.getNumAddresses() + " undefinedOutside=" + undef.getNumAddresses());
    }
}
