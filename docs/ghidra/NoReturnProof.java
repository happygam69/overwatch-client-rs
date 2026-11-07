// Read-only: evidence for and against each no-return flag. A function whose
// body contains a RET instruction can return on that path, so the flag is
// wrong for it. Prints one line per flag: address, name, body size, call sites
// with a continuation, and whether the body contains RET / UD2 / INT3.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.symbol.ReferenceIterator;
import ghidra.program.model.symbol.SourceType;

import java.util.ArrayList;
import java.util.List;

public class NoReturnProof extends GhidraScript {
    @Override
    public void run() throws Exception {
        FunctionManager fm = currentProgram.getFunctionManager();
        Listing listing = currentProgram.getListing();
        List<String> withRet = new ArrayList<String>();
        List<String> noRet = new ArrayList<String>();
        int total = 0;
        long sitesTotal = 0;
        for (Function f : fm.getFunctions(true)) {
            if (!f.hasNoReturn()) {
                continue;
            }
            total++;
            boolean ret = false, ud2 = false, trace = false;
            int insn = 0;
            for (Instruction ins : listing.getInstructions(new AddressSet(f.getBody()), true)) {
                insn++;
                String m = ins.getMnemonicString();
                if (m.startsWith("RET")) {
                    ret = true;
                }
                if (m.startsWith("UD2")) {
                    ud2 = true;
                }
                if (m.startsWith("INT3") || m.startsWith("INT")) {
                    trace = true;
                }
            }
            int cont = 0;
            ReferenceIterator ri =
                currentProgram.getReferenceManager().getReferencesTo(f.getEntryPoint());
            while (ri.hasNext()) {
                Address from = ri.next().getFromAddress();
                Function caller = fm.getFunctionContaining(from);
                if (caller == null || caller == f) {
                    continue;
                }
                Instruction c = listing.getInstructionContaining(from);
                if (c != null && c.getFlowType().isCall()) {
                    sitesTotal++;
                    if (c.getFallThrough() != null) {
                        cont++;
                    }
                }
            }
            String line = String.format("%x/%s/insn=%d/ext=%d/calls=%d/ret=%b/ud2=%b/int=%b", f
                    .getEntryPoint().getOffset(), f.getName(), insn, f.getBody().getNumAddresses(),
                cont, ret, ud2, trace);
            if (ret) {
                withRet.add(line);
            }
            else {
                noRet.add(line);
            }
        }
        println(String.format("NORET_PROOF total=%d with_ret=%d without_ret=%d call_sites=%d", total,
            withRet.size(), noRet.size(), sitesTotal));
        println("WITH_RET " + withRet);
        println("WITHOUT_RET " + noRet);
        println("NO_RETURN_PROOF DONE");
    }
}
