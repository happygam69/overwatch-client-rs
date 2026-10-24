// Dump Ghidra's function ranges for a band to a CSV the field index can read.
//
// Ghidra's function list is the one place that resolves `.pdata` fragments and
// gives each function the entry the decompiler uses. The field index
// (`dev/field_index.py`) reads this CSV with --ghidra-functions; using the
// `.pdata` begins alone is wrong where a chained/fragmented entry starts in
// the middle of a function.
//
// Args (whitespace-separated): <start-hex> <end-hex> <output-csv>
// Example:
//   DumpFunctionRanges.java 0x140c00000 0x140e00000 C:/Temp/mover_functions.csv
//
// The output rows are: begin,end,name (end is the last address of the body).
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import java.io.FileWriter;
import java.io.PrintWriter;

public class DumpFunctionRanges extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 3) {
            println("usage: DumpFunctionRanges <start-hex> <end-hex> <output-csv>");
            return;
        }
        long lo = Long.decode(args[0]);
        long hi = Long.decode(args[1]);
        String path = args[2];
        FunctionManager fm = currentProgram.getFunctionManager();
        PrintWriter w = new PrintWriter(new FileWriter(path));
        int n = 0;
        for (Function f : fm.getFunctions(true)) {
            long a = f.getEntryPoint().getOffset();
            if (a < lo || a >= hi) {
                continue;
            }
            long b = f.getBody().getMaxAddress().getOffset();
            w.println(Long.toHexString(a) + "," + Long.toHexString(b) + "," + f.getName());
            n++;
        }
        w.close();
        println("wrote " + n + " functions to " + path);
    }
}
