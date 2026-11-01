// Disassemble exactly the real .pdata function bodies.
//
// A clean import with -noanalysis creates the function symbols from the PE's
// exception directory but leaves every body empty (0 instructions). This pass
// fills each real function body from its own .pdata extent. It never follows a
// byte-pattern heuristic and never creates a function at a chained fragment.
//
// Usage: analyzeHeadless ... -process <program> -noanalysis
//          -postScript DisassemblePdataFunctions.java
//
// Read/write to the program; it changes instructions and function bodies only.
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;

public class DisassemblePdataFunctions extends GhidraScript {
    @Override
    public void run() throws Exception {
        Memory mem = currentProgram.getMemory();
        long base = currentProgram.getImageBase().getOffset();
        MemoryBlock pd = mem.getBlock(".pdata");
        if (pd == null) {
            println("DISASM no .pdata block");
            return;
        }
        byte[] b = new byte[(int) Math.min(pd.getSize(), Integer.MAX_VALUE)];
        int got = mem.getBytes(pd.getStart(), b);
        FunctionManager fm = currentProgram.getFunctionManager();
        int real = 0, disassembled = 0, failed = 0, chained = 0;
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
            int flags = 0;
            try {
                flags = (mem.getByte(toAddr(base + unw)) & 0xff) >> 3 & 0x1f;
            }
            catch (Exception e) {
                // No unwind bytes: treat the entry as a real function.
            }
            Address begin = toAddr(base + beg);
            Address finish = toAddr(base + end);
            AddressSet body = new AddressSet(begin, finish.subtract(1));
            if ((flags & 4) != 0) {
                // A chained fragment is part of a larger function, not a
                // callable function. Disassemble it so the parent's jumps can
                // be followed, but never create a function at its start.
                chained++;
                try {
                    new DisassembleCommand(begin, body, true)
                        .applyTo(currentProgram, monitor);
                }
                catch (Exception e) {
                    failed++;
                }
                continue;
            }
            real++;
            Function f = fm.getFunctionAt(begin);
            if (f == null) {
                f = fm.createFunction(null, begin, body,
                    ghidra.program.model.symbol.SourceType.ANALYSIS);
            }
            if (f == null) {
                failed++;
                continue;
            }
            // The PE loader creates each function with an entry-only body.
            // The .pdata extent is the authoritative body; apply it before
            // disassembly so the decompiler sees the whole function.
            try {
                f.setBody(body);
            }
            catch (Exception e) {
                failed++;
                continue;
            }
            try {
                boolean ok = new DisassembleCommand(begin, body, true)
                    .applyTo(currentProgram, monitor);
                if (ok) {
                    disassembled++;
                }
                else {
                    failed++;
                }
            }
            catch (Exception e) {
                failed++;
            }
            if ((real % 1000) == 0) {
                println("progress " + real + " disassembled=" + disassembled
                    + " failed=" + failed);
            }
            if (monitor.isCancelled()) {
                break;
            }
        }
        println("DISASM real=" + real + " disassembled=" + disassembled
            + " chained=" + chained + " failed=" + failed);
    }
}
