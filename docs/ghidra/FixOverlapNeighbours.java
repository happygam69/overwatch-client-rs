// Finish the 4 non-chained .pdata ranges whose over-followed neighbours still
// overlap them: trim neighbours below the range start, then create the
// .pdata function with its recorded extent.
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.address.AddressSetView;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.SourceType;
import java.util.TreeSet;

public class FixOverlapNeighbours extends GhidraScript {
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
            {0x5f7b40L,0x5f9566L},{0xbfbdd0L,0xbfe749L},{0xcf75f0L,0xcf7936L},{0x1e38870L,0x1e39216L}
        };
        int trimmed = 0, removed = 0, created = 0, failed = 0;
        for (long[] r : ranges) {
            Address lo = toAddr(base + r[0]);
            Address hi = toAddr(base + r[1]);
            AddressSet range = new AddressSet(lo, hi.subtract(1));
            java.util.List<Address> victims = new java.util.ArrayList<Address>();
            FunctionIterator fi = fm.getFunctions(true);
            while (fi.hasNext()) {
                Function f = fi.next();
                if (!f.getBody().intersects(range)) continue;
                long ep = f.getEntryPoint().getOffset();
                if (ep >= lo.getOffset() && ep < hi.getOffset()) {
                    if (!begins.contains(Long.valueOf(ep - base))) victims.add(f.getEntryPoint());
                } else if (ep < lo.getOffset()) {
                    AddressSet below = new AddressSet(f.getEntryPoint(), lo.subtract(1));
                    AddressSetView nb = f.getBody().intersect(below);
                    if (nb.isEmpty()) victims.add(f.getEntryPoint());
                    else { f.setBody(nb); trimmed++; }
                }
            }
            for (Address v : victims) { fm.removeFunction(v); removed++; }
            if (fm.getFunctionAt(lo) == null) {
                AddressSet body = new AddressSet(lo, hi.subtract(1));
                try {
                    new DisassembleCommand(lo, body, true).applyTo(currentProgram, monitor);
                    Function f = fm.createFunction(null, lo, body, SourceType.ANALYSIS);
                    if (f == null) { failed++; println(String.format("FAIL %s..%s", lo, hi)); }
                    else { created++; println(String.format("created %s..%s", lo, hi)); }
                } catch (Exception ex) {
                    failed++;
                    println(String.format("ERROR %s..%s %s", lo, hi, ex));
                }
            }
        }
        println("DONE trimmed=" + trimmed + " removed=" + removed + " created=" + created + " failed=" + failed);
    }
}
