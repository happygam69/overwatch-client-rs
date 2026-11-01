// Check docs/function-names.csv against a program.
//
// Usage:
//   analyzeHeadless ... -process <program> -noanalysis
//     -postScript CheckNames.java E:/overwatch-client-rs/docs/function-names.csv
//
// Reports rows whose address has no function, and rows whose Ghidra name is
// not the CSV name. Read-only.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolTable;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.List;

public class CheckNames extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: CheckNames <function-names.csv>");
            return;
        }
        Path path = Paths.get(args[0]);
        List<String> lines = Files.readAllLines(path, StandardCharsets.UTF_8);
        FunctionManager fm = currentProgram.getFunctionManager();
        SymbolTable st = currentProgram.getSymbolTable();
        int rows = 0, present = 0, labels = 0, missing = 0, wrong = 0;
        for (String line : lines) {
            if (line.isEmpty() || line.startsWith("program,")) {
                continue;
            }
            String[] col = line.split(",", -1);
            if (col.length < 4) {
                continue;
            }
            rows++;
            Address address;
            try {
                address = toAddr(Long.parseLong(col[1].replace("0x", ""), 16));
            }
            catch (Exception e) {
                missing++;
                println("BADADDR " + line);
                continue;
            }
            Function f = fm.getFunctionAt(address);
            if (f == null) {
                Symbol s = st.getPrimarySymbol(address);
                if (s != null && s.getName().equals(col[3])) {
                    labels++;
                }
                else {
                    missing++;
                    println("MISSING " + col[1] + " " + col[3]);
                }
            }
            else {
                present++;
                if (!f.getName().equals(col[3])) {
                    wrong++;
                    println("WRONG " + col[1] + " csv=" + col[3] + " ghidra=" + f.getName());
                }
            }
        }
        println("CHECK rows=" + rows + " present=" + present + " labels=" + labels
            + " missing=" + missing + " wrong=" + wrong);
    }
}
