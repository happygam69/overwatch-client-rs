// Create the genuinely missing non-chained .pdata functions.
// Chained (UNW_FLAG_CHAININFO) entries are fragments of a larger function and
// are deliberately not created.
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.SourceType;

public class RecoverRealPdataFunctions extends GhidraScript {
    @Override
    public void run() throws Exception {
        long base = 0x140000000L;
        long pdataVa = base + 0x450B000L;
        int n = 0x24D968 / 12;
        Memory mem = currentProgram.getMemory();
        FunctionManager fm = currentProgram.getFunctionManager();
        int created = 0, failed = 0;
        for (int i = 0; i < n; i++) {
            long off = pdataVa + i * 12L;
            long bs = Integer.toUnsignedLong(mem.getInt(toAddr(off)));
            long be = Integer.toUnsignedLong(mem.getInt(toAddr(off + 4)));
            long u = Integer.toUnsignedLong(mem.getInt(toAddr(off + 8)));
            if (bs == 0 || be <= bs || bs < 0x1000 || be > 0x2F2B3D3) continue;
            int flags = 0;
            try { flags = (mem.getByte(toAddr(base + u)) & 0xFF) >> 3 & 0x1F; } catch (Exception e) { continue; }
            if ((flags & 4) != 0) continue;  // chained fragment
            Address b = toAddr(base + bs);
            Address e = toAddr(base + be);
            if (fm.getFunctionAt(b) != null) continue;
            if (getFunctionContaining(b) != null) continue;
            if (getFunctionContaining(e.subtract(1)) != null) {
                println(String.format("overlap %#x..%#x", base + bs, base + be));
                failed++;
                continue;
            }
            AddressSet body = new AddressSet(b, e.subtract(1));
            try {
                new DisassembleCommand(b, body, true).applyTo(currentProgram, monitor);
                Function f = fm.createFunction(null, b, body, SourceType.ANALYSIS);
                if (f == null) { failed++; println(String.format("failed %#x..%#x", base + bs, base + be)); }
                else { created++; println(String.format("created %#x..%#x", base + bs, base + be)); }
            } catch (Exception ex) {
                failed++;
                println(String.format("error %#x..%#x %s", base + bs, base + be, ex));
            }
        }
        println("DONE created=" + created + " failed=" + failed);
    }
}
