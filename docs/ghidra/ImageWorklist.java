// Read-only worklists for the second pass of the image audit (sections 8.8,
// 8.10 and 8.11 of docs/ghidra-image-audit.md). Prints, in one run:
//
//   - the code targets of call/jump references that no function and no data
//     item covers, ranked by reference count (the `.pdata`-less callees);
//   - the 47 function names shared by two or more functions;
//   - signature provenance and parameter depth;
//   - the RTTI type-descriptor names and whether anything points at them.
//
// Modifies nothing. Writes C:/Temp/image_worklist.json for tooling.

import java.io.*;
import java.util.*;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.data.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;

public class ImageWorklist extends GhidraScript {

    @Override
    public void run() throws Exception {
        Listing lst = currentProgram.getListing();
        FunctionManager fm = currentProgram.getFunctionManager();
        ReferenceManager rm = currentProgram.getReferenceManager();
        Memory mem = currentProgram.getMemory();
        StringBuilder out = new StringBuilder("{\n");

        // ---- 8.8 code targets with no function and no data
        Map<Long, Integer> refs = new TreeMap<>();
        Map<Long, Address> addr = new TreeMap<>();
        for (Function f : fm.getFunctions(true)) {
            for (Instruction in : lst.getInstructions(f.getBody(), true)) {
                for (int i = 0; i < in.getNumOperands(); i++) {
                    for (Reference r : in.getOperandReferences(i)) {
                        Address t = r.getToAddress();
                        if (t == null || !t.isMemoryAddress() || !mem.contains(t)) continue;
                        if (fm.getFunctionContaining(t) != null) continue;
                        if (lst.getDataContaining(t) != null) continue;
                        addr.putIfAbsent(t.getOffset(), t);
                        refs.merge(t.getOffset(), 1, Integer::sum);
                    }
                }
            }
        }
        List<Long> keys = new ArrayList<>(refs.keySet());
        keys.sort((a, b) -> refs.get(b) - refs.get(a));
        int hot = 0;
        for (Long k : keys) if (refs.get(k) >= 10) hot++;
        out.append("\"functionless_targets\": ").append(keys.size()).append(",\n");
        out.append("\"functionless_targets_10plus\": ").append(hot).append(",\n");
        out.append("\"functionless_top\": [");
        for (int i = 0; i < Math.min(25, keys.size()); i++) {
            if (i > 0) out.append(",");
            out.append("\n {\"at\":\"").append(addr.get(keys.get(i))).append("\",\"refs\":")
               .append(refs.get(keys.get(i))).append("}");
        }
        out.append("],\n");

        // ---- 8.10 duplicate names, 8.11 signature provenance
        Map<String, List<String>> byName = new TreeMap<>();
        Map<String, Integer> sigSource = new TreeMap<>();
        int funcs = 0, paramsF = 0, paramsTotal = 0;
        for (Function f : fm.getFunctions(true)) {
            funcs++;
            byName.computeIfAbsent(f.getName(), k -> new ArrayList<>()).add(f.getEntryPoint().toString());
            sigSource.merge(String.valueOf(f.getSignatureSource()), 1, Integer::sum);
            if (f.getParameterCount() > 0) paramsF++;
            paramsTotal += f.getParameterCount();
        }
        int dup = 0;
        out.append("\"duplicate_names\": [");
        boolean first = true;
        for (Map.Entry<String, List<String>> e : byName.entrySet()) {
            if (e.getValue().size() < 2) continue;
            dup++;
            if (!first) out.append(",");
            first = false;
            out.append("\n {\"name\":\"").append(e.getKey().replace("\"", "'")).append("\",\"at\":")
               .append(e.getValue()).append("}");
        }
        out.append("],\n");
        out.append("\"functions\": ").append(funcs).append(",\n");
        out.append("\"duplicate_name_count\": ").append(dup).append(",\n");
        out.append("\"signature_source\": ").append(sigSource).append(",\n");
        out.append("\"functions_with_params\": ").append(paramsF).append(",\n");
        out.append("\"parameters_total\": ").append(paramsTotal).append(",\n");

        // ---- 8.9 RTTI type descriptors
        List<String> rtti = new ArrayList<>();
        int referenced = 0;
        Address cur = mem.getBlock(".data").getStart();
        while (rtti.size() < 200) {
            Address hit = mem.findBytes(cur, new byte[]{ '.', '?', 'A', 'V' }, null, true, monitor);
            if (hit == null || !mem.getBlock(hit).getName().equals(".data")) break;
            StringBuilder nm = new StringBuilder();
            for (int k = 0; k < 96; k++) {
                try {
                    byte b = mem.getByte(hit.add(k));
                    if (b == 0) break;
                    nm.append((char) (b & 0xff));
                } catch (Exception e) { break; }
            }
            int n = 0;
            for (Reference q : rm.getReferencesTo(hit)) n++;
            if (n > 0) referenced++;
            rtti.add(hit + " " + nm + " xrefs=" + n);
            cur = hit.add(1);
        }
        out.append("\"rtti_descriptors\": ").append(rtti.size()).append(",\n");
        out.append("\"rtti_with_absolute_xref\": ").append(referenced).append(",\n");
        out.append("\"rtti\": ").append(rtti.subList(0, Math.min(20, rtti.size()))).append("\n}\n");

        try (Writer w = new FileWriter("C:/Temp/image_worklist.json")) { w.write(out.toString()); }
        println("WORKLIST functionless=" + keys.size() + " (>=10 refs: " + hot + ")"
                + " dupNames=" + dup + " rtti=" + rtti.size() + "/" + referenced
                + " params=" + paramsF + "/" + paramsTotal);
    }
}
