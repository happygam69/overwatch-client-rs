// DecompScope.java - what body does the decompiler actually see?
//
// Usage: DecompScope.java <address> [more addresses...]
//
// For each address prints:
//   prog   the program's function body (bytes, ranges)
//   dec    the body the decompiler was handed (bytes, ranges)
//   c      length of the C text, line count, timed-out flag
// If `dec` is smaller than `prog`, the decompiler is working from a stale or
// clipped function and any comparison of its output before/after a body change
// is meaningless - fix the caller (flushCache / a fresh DecompInterface), not
// the analysis.
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class DecompScope extends GhidraScript {

    @Override
    public void run() throws Exception {
        DecompInterface di = new DecompInterface();
        di.toggleCCode(true);
        di.openProgram(currentProgram);
        for (String a : getScriptArgs()) {
            Address at = toAddr(a);
            Function f = currentProgram.getFunctionManager().getFunctionAt(at);
            if (f == null) {
                println("SCOPE " + a + ": no function");
                continue;
            }
            di.flushCache();
            DecompileResults r = di.decompileFunction(f, 120, monitor);
            String body = "none";
            if (r.getFunction() != null) {
                body = r.getFunction().getBody().getNumAddresses()
                    + "B/" + r.getFunction().getBody().getNumAddressRanges() + "R";
            }
            String c = r.getDecompiledFunction() == null ? "" : r.getDecompiledFunction().getC();
            println(String.format(
                "SCOPE %s prog=%dB/%dR dec=%s c=%dch/%dln completed=%s timeout=%s",
                a, f.getBody().getNumAddresses(), f.getBody().getNumAddressRanges(), body,
                c.length(), c.isEmpty() ? 0 : c.split("\n").length,
                r.decompileCompleted(), r.isTimedOut()));
        }
        di.dispose();
    }
}
