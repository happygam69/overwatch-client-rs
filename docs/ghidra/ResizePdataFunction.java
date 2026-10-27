// Resize one function to its .pdata extent and remove the inner fragments.
//
// The install's .pdata is authoritative: a chained/fragmented entry can be
// wider than the body Ghidra created, and Ghidra may have split the real
// function into several smaller functions. This script removes every function
// overlapping the requested range except the one at the begin, then sets that
// function's body to the .pdata range. Run it only on a range you have read
// from `.pdata`; it is a repair, not a heuristic.
//
// Args (whitespace-separated): <begin-hex> <end-hex>
// Example:
//   ResizePdataFunction.java 0x14068ae00 0x14068b333
//
// If Ghidra reports "ClassNotFoundException" after a previous script edit,
// restart Ghidra: its script class cache does not always pick up a changed
// file. The script changes the project only; save the program afterwards.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.symbol.SourceType;

public class ResizePdataFunction extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) {
            println("usage: ResizePdataFunction <begin-hex> <end-hex>");
            return;
        }
        long beginOffset = Long.decode(args[0]);
        long endOffset = Long.decode(args[1]);
        Address begin = toAddr(beginOffset);
        Address end = toAddr(endOffset);
        AddressSet range = new AddressSet(begin, end.subtract(1));
        FunctionManager fm = currentProgram.getFunctionManager();

        java.util.List<Address> victims = new java.util.ArrayList<Address>();
        FunctionIterator it = fm.getFunctionsOverlapping(range);
        while (it.hasNext()) {
            Function f = it.next();
            if (f.getEntryPoint().getOffset() != beginOffset) {
                victims.add(f.getEntryPoint());
            }
        }
        for (Address victim : victims) {
            fm.removeFunction(victim);
        }

        Function function = fm.getFunctionAt(begin);
        if (function == null) {
            new ghidra.app.cmd.disassemble.DisassembleCommand(begin, range, true)
                .applyTo(currentProgram, monitor);
            function = fm.createFunction(null, begin, range, SourceType.ANALYSIS);
        } else {
            function.setBody(range);
        }
        if (function == null) {
            println("FAIL: no function at " + begin);
        } else {
            println("function " + function.getName()
                + " body " + function.getBody().getMinAddress()
                + ".." + function.getBody().getMaxAddress()
                + " (removed " + victims.size() + " inner fragment(s))");
        }
    }
}
