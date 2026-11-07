// Read-only: every function carrying the no-return flag, with its size, name
// source and how many of its call sites have a defined continuation inside the
// caller (a false-flag signature). Also counts name provenance in the program.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.SourceType;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

public class NoReturnList extends GhidraScript {
    @Override
    public void run() throws Exception {
        FunctionManager fm = currentProgram.getFunctionManager();
        Listing listing = currentProgram.getListing();
        MemoryBlock text = currentProgram.getMemory().getBlock(".text");
        HashMap<String, Integer> sites = new HashMap<String, Integer>();
        for (Instruction ins : listing.getInstructions(new AddressSet(text.getStart(), text.getEnd()),
            true)) {
            if (!ins.getFlowType().isCall()) {
                continue;
            }
            for (Address t : ins.getFlows()) {
                Function cf = fm.getFunctionAt(t);
                if (cf == null || !cf.hasNoReturn()) {
                    continue;
                }
                Address fall = ins.getFallThrough();
                Function owner = fm.getFunctionContaining(ins.getAddress());
                if (fall == null || owner == null || fm.getFunctionContaining(fall) != owner) {
                    continue;
                }
                String k = cf.getEntryPoint().toString();
                Integer c = sites.get(k);
                sites.put(k, c == null ? 1 : c + 1);
            }
        }
        List<Function> flagged = new ArrayList<Function>();
        int fid = 0, lib = 0, user = 0, anon = 0;
        for (Function f : fm.getFunctions(true)) {
            String n = f.getName();
            if (n.startsWith("FID_conflict:") || n.startsWith("FID_")) {
                fid++;
            }
            else if (!n.startsWith("FUN_") && !n.startsWith("thunk_")) {
                lib++;
            }
            else {
                anon++;
            }
            if (f.hasNoReturn()) {
                flagged.add(f);
            }
        }
        println(String.format("NAMES fid=%d nonfun=%d fun_or_thunk=%d", fid, lib, anon));
        for (Function f : flagged) {
            String k = f.getEntryPoint().toString();
            Integer c = sites.get(k);
            println(String.format("NR %s %s size=%d proto=%s cont_sites=%d", k, f.getName(),
                f.getBody().getNumAddresses(), f.getSignatureSource() == SourceType.USER_DEFINED
                        ? "user" : f.getSignatureSource().toString(),
                c == null ? 0 : c));
        }
        println(String.format("NR_TOTAL %d", flagged.size()));
        println("NORETURN_LIST DONE");
    }
}
