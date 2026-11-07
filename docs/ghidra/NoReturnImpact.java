// Read-only: how much decompiled code a wrong no-return flag hides.
// A flag is treated as wrong when the flagged function's own body contains a
// RET instruction (so it can return on that path). For every call instruction
// whose target is such a function, the caller's next instruction is recorded;
// if it exists inside the caller, the decompiler's view of the caller ends at
// the call and the rest is dead code.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.MemoryBlock;

import java.util.HashMap;
import java.util.HashSet;

public class NoReturnImpact extends GhidraScript {
    @Override
    public void run() throws Exception {
        FunctionManager fm = currentProgram.getFunctionManager();
        Listing listing = currentProgram.getListing();
        MemoryBlock text = currentProgram.getMemory().getBlock(".text");
        HashSet<Long> falseFlags = new HashSet<Long>();
        HashSet<Long> trueFlags = new HashSet<Long>();
        for (Function f : fm.getFunctions(true)) {
            if (!f.hasNoReturn()) {
                continue;
            }
            boolean ret = false;
            for (Instruction ins : listing.getInstructions(new AddressSet(f.getBody()), true)) {
                if (ins.getMnemonicString().startsWith("RET")) {
                    ret = true;
                    break;
                }
            }
            (ret ? falseFlags : trueFlags).add(f.getEntryPoint().getOffset());
        }
        long falseCalls = 0, trueCalls = 0, falseHiddenIns = 0;
        HashSet<Long> falseCallers = new HashSet<Long>();
        HashMap<Long, Long> hiddenPerCaller = new HashMap<Long, Long>();
        for (Instruction ins : listing.getInstructions(
            new AddressSet(text.getStart(), text.getEnd()), true)) {
            if (!ins.getFlowType().isCall()) {
                continue;
            }
            Function caller = fm.getFunctionContaining(ins.getAddress());
            if (caller == null) {
                continue;
            }
            for (Address t : ins.getFlows()) {
                long k = t.getOffset();
                boolean isFalse = falseFlags.contains(k);
                if (!isFalse && !trueFlags.contains(k)) {
                    continue;
                }
                if (isFalse) {
                    falseCalls++;
                    falseCallers.add(caller.getEntryPoint().getOffset());
                }
                else {
                    trueCalls++;
                }
                Address next = ins.getFallThrough();
                // count the instructions of this caller that sit after the call
                // (only for false flags: they are the hidden remainder)
                if (isFalse && next != null
                    && next.compareTo(caller.getBody().getMaxAddress()) <= 0) {
                    long n = 0;
                    for (Instruction x : listing.getInstructions(
                        new AddressSet(next, caller.getBody().getMaxAddress()), true)) {
                        n++;
                    }
                    Long prev = hiddenPerCaller.get(caller.getEntryPoint().getOffset());
                    hiddenPerCaller.put(caller.getEntryPoint().getOffset(),
                        prev == null ? n : Math.max(prev, n));
                }
            }
        }
        for (Long v : hiddenPerCaller.values()) {
            falseHiddenIns += v;
        }
        println(String.format(
            "IMPACT false_flags=%d true_flags=%d call_to_false=%d callers=%d hidden_instructions_upper_bound=%d call_to_true=%d",
            falseFlags.size(), trueFlags.size(), falseCalls, falseCallers.size(), falseHiddenIns,
            trueCalls));
        println("IMPACT DONE");
    }
}
