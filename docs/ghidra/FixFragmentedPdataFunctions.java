// Fix the 11 non-chained .pdata ranges that pass 2/3 had fragmented with
// pointer-target functions inside them. Removes those non-.pdata artifacts,
// then creates the real .pdata function with its recorded extent.
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.SourceType;
import java.util.TreeSet;

public class FixFragmentedPdataFunctions extends GhidraScript {
    @Override
    public void run() throws Exception {
        long base = 0x140000000L;
        long pdataVa = base + 0x450B000L;
        int n = 0x24D968 / 12;
        Memory mem = currentProgram.getMemory();
        FunctionManager fm = currentProgram.getFunctionManager();
        TreeSet<Long> begins = new TreeSet<Long>();
        for (int i = 0; i < n; i++) {
            long bs = Integer.toUnsignedLong(mem.getInt(toAddr(pdataVa + i * 12L)));
            long be = Integer.toUnsignedLong(mem.getInt(toAddr(pdataVa + i * 12L + 4)));
            if (bs != 0 && be > bs) begins.add(bs);
        }
        long[][] ranges = {
            {0x5f7b40L,0x5f9566L},{0x61fef0L,0x620401L},{0x97bc30L,0x97c0b5L},
            {0xbfbdd0L,0xbfe749L},{0xcf75f0L,0xcf7936L},{0x1c55770L,0x1c55aa1L},
            {0x1e182b0L,0x1e18557L},{0x1e34fa0L,0x1e359a5L},{0x1e38870L,0x1e39216L},
            {0x1e3be10L,0x1e3ca7bL},{0x2e5f320L,0x2e5f488L}
        };
        int deleted = 0, created = 0, failed = 0;
        for (long[] r : ranges) {
            long lo = base + r[0], hi = base + r[1];
            // remove non-.pdata functions inside the range
            java.util.List<Address> victims = new java.util.ArrayList<Address>();
            FunctionIterator fi = fm.getFunctions(true);
            while (fi.hasNext()) {
                Function f = fi.next();
                long ep = f.getEntryPoint().getOffset();
                if (ep >= lo && ep < hi && !begins.contains(Long.valueOf(ep - base)))
                    victims.add(f.getEntryPoint());
            }
            for (Address v : victims) {
                fm.removeFunction(v);
                deleted++;
            }
            Address b = toAddr(lo);
            Address e = toAddr(hi);
            AddressSet body = new AddressSet(b, e.subtract(1));
            try {
                new DisassembleCommand(b, body, true).applyTo(currentProgram, monitor);
                Function f = fm.createFunction(null, b, body, SourceType.ANALYSIS);
                if (f == null) { failed++; println(String.format("FAIL %#x..%#x", lo, hi)); }
                else { created++; println(String.format("created %#x..%#x (deleted %d)", lo, hi, victims.size())); }
            } catch (Exception ex) {
                failed++;
                println(String.format("ERROR %#x..%#x %s", lo, hi, ex));
            }
        }
        println("DONE deleted=" + deleted + " created=" + created + " failed=" + failed);
    }
}
