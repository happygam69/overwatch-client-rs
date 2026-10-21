import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.symbol.SourceType;

import java.io.BufferedWriter;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.List;

/**
 * Canonical headless decompiler.
 *
 * Usage (from analyzeHeadless -postScript, arguments split on whitespace, so
 * paths must not contain spaces):
 *
 *   -postScript McpDecompile.java &lt;args-file&gt; &lt;out-file&gt; [timeout-sec]
 *
 * The args file is whitespace-separated `start end` pairs in the program's
 * own address space: for a stripped import give `rva - 0x10000`, for a PE
 * import give `image base + rva`. Lines may carry `#` comments (whole line
 * only). For each pair: reuse the function at `start` if Ghidra has one,
 * otherwise disassemble [start,end) and create it; decompile with the given
 * timeout (default 60 s); write a `// ===== sub_<start> =====` header and the
 * C. Reports `created=<n> failed=<n> -> out`.
 *
 * This replaces the per-run McpAnimChainN copies.
 */
public class McpDecompile extends GhidraScript {
    public void run() throws Exception {
        String[] argv = getScriptArgs();
        if (argv.length < 2) {
            println("usage: McpDecompile <args-file> <out-file> [timeout-sec]");
            return;
        }
        String argPath = argv[0];
        String outPath = argv[1];
        int timeout = argv.length > 2 ? Integer.parseInt(argv[2]) : 60;

        List<String> tok = new ArrayList<String>();
        for (String t : new String(
                java.nio.file.Files.readAllBytes(java.nio.file.Paths.get(argPath)),
                java.nio.charset.StandardCharsets.UTF_8).split("\\s+")) {
            if (t.length() > 0 && !t.startsWith("#")) tok.add(t);
        }

        DecompInterface di = new DecompInterface();
        di.openProgram(currentProgram);
        PrintWriter out = new PrintWriter(new BufferedWriter(new FileWriter(outPath)));
        FunctionManager fm = currentProgram.getFunctionManager();
        int created = 0, failed = 0;
        for (int i = 0; i + 1 < tok.size(); i += 2) {
            long beg = Long.decode(tok.get(i));
            long end = Long.decode(tok.get(i + 1));
            Address B = toAddr(beg);
            Address E = toAddr(end);
            Function f = fm.getFunctionAt(B);
            if (f == null) {
                new DisassembleCommand(B, new AddressSet(B, E.subtract(1)), true)
                    .applyTo(currentProgram, monitor);
                f = fm.createFunction(null, B, new AddressSet(B, E.subtract(1)),
                    SourceType.USER_DEFINED);
            }
            if (f == null) { failed++; continue; }
            created++;
            out.println("// ===== sub_" + Long.toHexString(beg) + " =====");
            DecompileResults r = di.decompileFunction(f, timeout, monitor);
            if (r.decompileCompleted() && r.getDecompiledFunction() != null) {
                out.println(r.getDecompiledFunction().getC());
            } else {
                out.println("// decompile failed: "
                    + (r.getErrorMessage() == null ? "?" : r.getErrorMessage()));
            }
        }
        out.close();
        println("created=" + created + " failed=" + failed + " -> " + outPath);
    }
}
