// ProbeFlow.java - print why a decompiler stops where it stops.
//
// Usage: ProbeFlow.java <address>
//
// Prints the function's body ranges and the last <n> instructions with their
// flow type and target, so a decompile that ends early can be explained by the
// code itself (a ret, a jump out of the body, an undefined byte) rather than by
// guessing from the C text.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressRange;
import ghidra.program.model.address.AddressRangeIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.symbol.FlowType;

public class ProbeFlow extends GhidraScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: ProbeFlow.java <address> [tail-count]");
            return;
        }
        Address at = toAddr(args[0]);
        int tail = args.length > 1 ? Integer.parseInt(args[1]) : 14;
        Function f = currentProgram.getFunctionManager().getFunctionAt(at);
        if (f == null) {
            println("PROBE " + at + ": no function");
            return;
        }
        StringBuilder ranges = new StringBuilder();
        AddressRangeIterator it = f.getBody().getAddressRanges();
        int nr = 0;
        while (it.hasNext() && nr < 12) {
            AddressRange r = it.next();
            ranges.append(String.format("%x..%x ", r.getMinAddress().getOffset(),
                r.getMaxAddress().getOffset() + 1));
            nr++;
        }
        println(String.format("PROBE %x body=%d ranges=%d [%s]",
            at.getOffset(), f.getBody().getNumAddresses(), f.getBody().getNumAddressRanges(),
            ranges.toString().trim()));

        // Walk the body from the entry and keep the last N instructions, with
        // the flow type of each and the target when the flow leaves the body.
        Instruction ins = currentProgram.getListing().getInstructionAt(f.getEntryPoint());
        StringBuilder tailText = new StringBuilder();
        int seen = 0, undefined = 0, leftBody = 0;
        Address cur = f.getEntryPoint();
        while (cur != null && f.getBody().contains(cur)) {
            ins = currentProgram.getListing().getInstructionAt(cur);
            if (ins == null) {
                undefined++;
                cur = cur.add(1);
                continue;
            }
            FlowType ft = ins.getFlowType();
            String extra = "";
            if (!ft.isFallthrough() && !ft.isTerminal()) {
                Address[] flows = ins.getFlows();
                if (flows.length > 0 && !f.getBody().contains(flows[0])) {
                    leftBody++;
                    extra = " TARGET_OUTSIDE_BODY=" + flows[0];
                }
            }
            tailText.append(String.format("  %x %-28s %s%s%n", cur.getOffset(),
                ins.toString(), ft, extra));
            seen++;
            if (tailText.length() > 4000) {
                tailText.delete(0, tailText.length() - 3000);
            }
            Address next = ins.getFallThrough();
            if (next == null) {
                break;
            }
            cur = next;
        }
        String[] lines = tailText.toString().split("\n");
        int from = Math.max(0, lines.length - tail);
        for (int i = from; i < lines.length; i++) {
            println(lines[i]);
        }
        println(String.format("PROBE %x linear=%d undefined_in_path=%d out_of_body_targets=%d",
            at.getOffset(), seen, undefined, leftBody));
    }
}
