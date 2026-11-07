// Clear the `noreturn` flags that are provably wrong, and restore them again.
//
//   ClearFalseNoReturn.java [clear|restore]
//
// A flag is provably wrong when the flagged function's own body contains a RET
// instruction: that path returns to the caller. Clearing it restores the code
// the decompiler treated as dead after every call to that function (see
// docs/ghidra-image-audit.md, section 5.1: 110 flags, 11,552 call sites in
// 6,355 functions).
//
// The script writes the cleared addresses to C:/Temp/noreturn_cleared.txt, so
// `restore` can put the flags back byte for byte. It never touches a function
// that has no RET - a flag without a RET may be genuine (the CFG dispatch
// thunk is one of them) and is left for a reader to judge.
//
// Run ConfigureQualityAnalysis.java first (or with it), so
// `Non-Returning Functions - Known` cannot re-apply the flags on the next
// analysis pass.
import java.io.*;
import java.util.*;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;

public class ClearFalseNoReturn extends GhidraScript {
    static final String LIST = "C:/Temp/noreturn_cleared.txt";

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        StringBuilder letters = new StringBuilder();
        if (args.length > 0) for (char c : args[0].toCharArray()) if (Character.isLetter(c)) letters.append(c);
        String mode = letters.length() == 0 ? "clear" : letters.toString();

        List<Function> flagged = new ArrayList<>();
        for (Function f : currentProgram.getFunctionManager().getFunctions(true)) if (f.hasNoReturn()) flagged.add(f);
        println("NORETURN flagged_before=" + flagged.size() + " mode=" + mode);

        if (mode.equals("restore")) {
            int restored = 0, already = 0;
            for (String line : java.nio.file.Files.readAllLines(new File(LIST).toPath())) {
                line = line.trim();
                if (line.isEmpty()) continue;
                Address ad = toAddr(Long.parseLong(line.replace("0x", ""), 16));
                Function f = getFunctionAt(ad);
                if (f == null) { println("RESTORE missing function " + ad); continue; }
                if (f.hasNoReturn()) { already++; continue; }
                f.setNoReturn(true);
                restored++;
            }
            println("RESTORE restored=" + restored + " already_flagged=" + already
                + " flagged_after=" + countFlagged());
            return;
        }

        int cleared = 0, keptNoRet = 0;
        List<String> list = new ArrayList<>();
        for (Function f : flagged) {
            boolean hasRet = false;
            for (Instruction in : currentProgram.getListing().getInstructions(f.getBody(), true)) {
                if (in.getMnemonicString().startsWith("RET")) { hasRet = true; break; }
            }
            if (!hasRet) { keptNoRet++; continue; }
            String ad = f.getEntryPoint().toString();
            f.setNoReturn(false);
            cleared++;
            list.add(ad);
        }
        try (Writer w = new FileWriter(LIST)) {
            for (String s : list) w.write(s + "\n");
        }
        println("NORETURN cleared=" + cleared + " kept_no_ret=" + keptNoRet
            + " flagged_after=" + countFlagged() + " list=" + LIST);
    }

    int countFlagged() {
        int n = 0;
        for (Function f : currentProgram.getFunctionManager().getFunctions(true)) if (f.hasNoReturn()) n++;
        return n;
    }
}
