// Snapshot every function's body so a repair pass can be diffed exactly.
//
//   BodySnapshot.java [tag]
//
// Writes one line per function, `entry body_bytes range_count`, to
// C:/Temp/owaudit/body_<tag>.txt (tag defaults to `now`). Run it before and
// after any pass that changes bodies, flags or options and diff the two files:
//
//   diff body_before.txt body_after.txt
//
// A repair that is meant to change bodies (merging chained continuations) moves
// thousands of lines on purpose; anything else must move none except the
// functions it was aimed at. This is how the pass on 2026-11-04 found that
// three bodies outside the repaired set had been clipped - see section 11 of
// docs/ghidra-image-audit.md.
//
// Read-only.
import java.io.*;
import java.util.*;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;

public class BodySnapshot extends GhidraScript {
    @Override
    public void run() throws Exception {
        String tag = "now";
        String[] args = getScriptArgs();
        if (args.length > 0) {
            StringBuilder s = new StringBuilder();
            for (char c : args[0].toCharArray()) if (Character.isLetterOrDigit(c) || c == '_' || c == '-') s.append(c);
            if (s.length() > 0) tag = s.toString();
        }
        FunctionManager fm = currentProgram.getFunctionManager();
        StringBuilder sb = new StringBuilder();
        for (Function f : fm.getFunctions(true)) {
            sb.append(f.getEntryPoint()).append(' ')
              .append(f.getBody().getNumAddresses()).append(' ')
              .append(f.getBody().getNumAddressRanges()).append('\n');
        }
        String path = "C:/Temp/owaudit/body_" + tag + ".txt";
        try (Writer w = new FileWriter(path)) { w.write(sb.toString()); }
        println("BODY SNAPSHOT functions=" + fm.getFunctionCount() + " file=" + path);
    }
}
