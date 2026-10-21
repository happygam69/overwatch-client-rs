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
import java.util.LinkedHashSet;
import java.util.Set;

public class McpCallers extends GhidraScript {
    public void run() throws Exception {
        String outPath = "D:/ow-dumps/anim_callers.c";
        String argPath = "D:/ow-dumps/caller_args_s.txt";
        java.util.List<String> a = new java.util.ArrayList<String>();
        for (String t : new String(java.nio.file.Files.readAllBytes(
                java.nio.file.Paths.get(argPath)),
                java.nio.charset.StandardCharsets.UTF_8).split("\\s+")) {
            if (t.length() > 0) a.add(t);
        }
        DecompInterface di = new DecompInterface();
        di.openProgram(currentProgram);
        PrintWriter out = new PrintWriter(new BufferedWriter(new FileWriter(outPath)));
        FunctionManager fm = currentProgram.getFunctionManager();
        int total = 0;
        Set<Long> seen = new LinkedHashSet<Long>();
        for (int i = 0; i + 1 < a.size(); i += 2) {
            long beg = Long.decode(a.get(i));
            long end = Long.decode(a.get(i + 1));
            Address B = toAddr(beg);
            Address E = toAddr(end);
            Function f = fm.getFunctionAt(B);
            if (f == null) {
                new DisassembleCommand(B, new AddressSet(B, E.subtract(1)), true)
                    .applyTo(currentProgram, monitor);
                f = fm.createFunction(null, B, new AddressSet(B, E.subtract(1)),
                    SourceType.USER_DEFINED);
            }
            if (f == null) continue;
            out.println("// ############ target " + Long.toHexString(beg) + " ("
                + f.getName() + ") ############");
            Set<Function> callers = f.getCallingFunctions(monitor);
            out.println("// callers: " + callers.size());
            int n = 0;
            for (Function c : callers) {
                long cb = c.getEntryPoint().getOffset();
                out.println("// caller " + Long.toHexString(cb) + " \""
                    + c.getName() + "\" size "
                    + c.getBody().getNumAddresses());
                if (!seen.add(cb)) continue;
                if (n >= 8) continue;
                n++;
                out.println("// ===== caller_sub_" + Long.toHexString(cb) + " =====");
                DecompileResults r = di.decompileFunction(c, 60, monitor);
                if (r.decompileCompleted() && r.getDecompiledFunction() != null) {
                    out.println(r.getDecompiledFunction().getC());
                } else {
                    out.println("// decompile failed: "
                        + (r.getErrorMessage() == null ? "?" : r.getErrorMessage()));
                }
                total++;
            }
        }
        out.close();
        println("decompiled callers=" + total + " -> " + outPath);
    }
}
