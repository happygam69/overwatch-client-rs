// Create the no-.pdata leaf names that the clean map does not contain.
//
// The clean map is exactly the PE's real .pdata functions. Some named client
// functions (small getters/setters) have no unwind entry. If the address is
// already inside a real function it is a local entry point: create a label
// there, not a nested function. Otherwise create a function in the gap up to
// the next real function, capped at 64 bytes.
//
// Usage:
//   analyzeHeadless ... -process <program> -noanalysis
//     -postScript CreateMissingNames.java E:/overwatch-client-rs/docs/function-names.csv
//
// Read/write to the program.
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolTable;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.List;

public class CreateMissingNames extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: CreateMissingNames <function-names.csv>");
            return;
        }
        Path path = Paths.get(args[0]);
        List<String> lines = Files.readAllLines(path, StandardCharsets.UTF_8);
        FunctionManager fm = currentProgram.getFunctionManager();
        SymbolTable st = currentProgram.getSymbolTable();
        int rows = 0, existing = 0, labels = 0, functions = 0, failed = 0;
        for (String line : lines) {
            if (line.isEmpty() || line.startsWith("program,")) {
                continue;
            }
            String[] col = line.split(",", -1);
            if (col.length < 6) {
                continue;
            }
            rows++;
            Address address;
            try {
                address = toAddr(Long.parseLong(col[1].replace("0x", ""), 16));
            }
            catch (Exception e) {
                failed++;
                continue;
            }
            if (fm.getFunctionAt(address) != null) {
                existing++;
                continue;
            }
            String name = col[3];
            String role = col[4];
            Function containing = fm.getFunctionContaining(address);
            if (containing != null) {
                try {
                    Symbol s = st.createLabel(address, name, SourceType.USER_DEFINED);
                    if (s != null) {
                        labels++;
                    }
                    else {
                        failed++;
                    }
                }
                catch (Exception e) {
                    failed++;
                }
                continue;
            }
            Function next = null;
            FunctionIterator forward = fm.getFunctions(address, true);
            while (forward.hasNext()) {
                Function candidate = forward.next();
                if (candidate.getEntryPoint().compareTo(address) > 0) {
                    next = candidate;
                    break;
                }
            }
            Address end = address.add(63);
            if (next != null) {
                Address before = next.getEntryPoint().subtract(1);
                if (before.compareTo(address) < 0) {
                    failed++;
                    continue;
                }
                if (before.compareTo(end) < 0) {
                    end = before;
                }
            }
            AddressSet body = new AddressSet(address, end);
            try {
                new DisassembleCommand(address, body, true).applyTo(currentProgram, monitor);
                Function f = fm.createFunction(null, address, body, SourceType.ANALYSIS);
                if (f == null) {
                    failed++;
                    continue;
                }
                f.setName(name, SourceType.USER_DEFINED);
                if (role != null && !role.isEmpty()) {
                    f.setComment(role);
                }
                functions++;
            }
            catch (Exception e) {
                failed++;
            }
        }
        println("CREATE rows=" + rows + " existing=" + existing + " labels=" + labels
            + " functions=" + functions + " failed=" + failed);
    }
}
