// Recovery pass 1: create the .pdata functions Ghidra's analysis skipped.
// Chained unwind entries (UNW_FLAG_CHAININFO) are fragments of a larger
// function and are skipped - they are not callable functions.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.SourceType;
import ghidra.app.cmd.disassemble.DisassembleCommand;

public class RecoverPdataFunctions extends GhidraScript {
    @Override
    public void run() throws Exception {
        long base = 0x140000000L;
        long pdataVa = base + 0x450B000L;
        int n = 0x24D968 / 12;
        Memory mem = currentProgram.getMemory();
        FunctionManager fm = currentProgram.getFunctionManager();
        int created = 0, failed = 0, skippedInside = 0, skippedChained = 0;
        for (int i = 0; i < n; i++) {
            if ((i & 0x3FF) == 0) {
                if (monitor.isCancelled()) break;
                println("progress " + i + "/" + n + " created=" + created);
            }
            long off = pdataVa + i * 12L;
            long bs = Integer.toUnsignedLong(mem.getInt(toAddr(off)));
            long be = Integer.toUnsignedLong(mem.getInt(toAddr(off + 4)));
            long u = Integer.toUnsignedLong(mem.getInt(toAddr(off + 8)));
            if (bs == 0 || be <= bs || bs < 0x1000 || be > 0x2F2B3D3) continue;
            int flags = 0;
            try { flags = (mem.getByte(toAddr(base + u)) & 0xFF) >> 3 & 0x1F; } catch (Exception e) { continue; }
            if ((flags & 4) != 0) { skippedChained++; continue; }
            Address b = toAddr(base + bs);
            Address e = toAddr(base + be);
            if (fm.getFunctionAt(b) != null) continue;
            if (getFunctionContaining(b) != null) { skippedInside++; continue; }
            if (getFunctionContaining(e.subtract(1)) != null) { skippedInside++; continue; }
            AddressSet body = new AddressSet(b, e.subtract(1));
            try {
                new DisassembleCommand(b, body, true).applyTo(currentProgram, monitor);
                Function f = fm.createFunction(null, b, body, SourceType.ANALYSIS);
                if (f == null) failed++;
                else created++;
            } catch (Exception ex) {
                failed++;
            }
        }
        println("DONE created=" + created + " failed=" + failed + " skippedInside=" + skippedInside
            + " skippedChained=" + skippedChained);
    }
}
