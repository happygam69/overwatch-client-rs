// Create functions for referenced code targets that Ghidra's analysis skipped.
//
//   RecoverPointerFunctions.java <mode> [limit]
//
// mode: int3   - only targets preceded by INT3 padding (conservative)
//       padded - also targets preceded by RET (0xC3) or zero padding
//       all    - every referenced target still outside a function
//
// limit: stop after this many creations (0 or omitted = all).
//
// Idempotent: targets already inside a function are skipped.
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
        long tlo = 0x1000, thi = 0x2F2B3D3;
        String[] args = getScriptArgs();
        String mode = args.length > 0 ? args[0] : "all";
        int limit = args.length > 1 ? Integer.parseInt(args[1]) : 0;
        Memory mem = currentProgram.getMemory();
        FunctionManager fm = currentProgram.getFunctionManager();
        java.util.HashSet<Long> tgts = new java.util.HashSet<Long>();
        for (MemoryBlock b : mem.getBlocks()) {
            String n = b.getName();
            if (!n.equals(".rdata") && !n.equals(".data")) continue;
            for (Address a = b.getStart(); a.add(7).compareTo(b.getEnd()) <= 0; a = a.add(8)) {
                long v = mem.getLong(a);
                if (v >= base + tlo && v < base + thi) tgts.add(v - base);
            }
        }
        int created = 0, failed = 0, considered = 0;
        long largest = 0;
        Address largestAt = null;
        for (long r : tgts) {
            if (limit > 0 && considered >= limit) break;
            Address ad = toAddr(base + r);
            if (fm.getFunctionAt(ad) != null) continue;
            if (getFunctionContaining(ad) != null) continue;
            int prev = -1;
            try { prev = mem.getByte(ad.subtract(1)) & 0xFF; } catch (Exception e) { }
            boolean noInstruction = getInstructionAt(ad) == null;
            if (mode.equals("int3") && prev != 0xCC) continue;
            if (mode.equals("padded") && !(prev == 0xCC || prev == 0xC3 || prev == 0x00)) continue;
            considered++;
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
        println("DONE mode=" + mode + " considered=" + considered + " created=" + created
            + " failed=" + failed + " largest=" + largest + " at " + largestAt);
    }
}
