// Export the live project's function annotations as a TSV for migration.
//
// Columns:
//   address  name  name_source  plate_b64  tags  no_return  prototype_b64  params_b64
//
// Only functions carrying a user name, plate comment, tag, no-return flag,
// user prototype or named parameter are written. Base64 keeps comments and
// prototypes with tabs/newlines safe.
//
// Usage:
//   analyzeHeadless ... -process <program> -noanalysis
//     -postScript ExportAnnotations.java C:/Temp/opencode/ow_annotations.tsv
//
// Read-only against the program; the TSV is outside the repository.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.FunctionTag;
import ghidra.program.model.listing.Parameter;
import ghidra.program.model.symbol.SourceType;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Base64;

public class ExportAnnotations extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String out = args.length > 0 ? args[0] : "C:/Temp/opencode/ow_annotations.tsv";
        FunctionManager fm = currentProgram.getFunctionManager();
        StringBuilder sb = new StringBuilder();
        sb.append("address\tname\tname_source\tplate_b64\ttags\tno_return\tprototype_b64\tparams_b64\n");
        int rows = 0;
        for (Function f : fm.getFunctions(true)) {
            boolean user = f.getSymbol().getSource() == SourceType.USER_DEFINED;
            String plate = f.getComment();
            StringBuilder tags = new StringBuilder();
            for (FunctionTag t : f.getTags()) {
                if (tags.length() > 0) {
                    tags.append(';');
                }
                tags.append(t.getName());
            }
            boolean noReturn = f.hasNoReturn();
            String proto = f.getSignatureSource() == SourceType.USER_DEFINED
                ? f.getPrototypeString(true, false) : "";
            StringBuilder ps = new StringBuilder();
            for (Parameter p : f.getParameters()) {
                String pn = p.getName();
                if (pn != null && !pn.startsWith("param_") && !pn.equals("this")) {
                    if (ps.length() > 0) {
                        ps.append(';');
                    }
                    ps.append(p.getOrdinal()).append(':').append(pn);
                }
            }
            if (!user && (plate == null || plate.isEmpty()) && tags.length() == 0
                && !noReturn && proto.isEmpty() && ps.length() == 0) {
                continue;
            }
            String plate64 = Base64.getEncoder().encodeToString(
                (plate == null ? "" : plate).getBytes(StandardCharsets.UTF_8));
            String proto64 = Base64.getEncoder().encodeToString(
                proto.getBytes(StandardCharsets.UTF_8));
            String ps64 = Base64.getEncoder().encodeToString(
                ps.toString().getBytes(StandardCharsets.UTF_8));
            sb.append(f.getEntryPoint()).append('\t').append(f.getName()).append('\t')
                .append(user ? "USER" : "OTHER").append('\t').append(plate64).append('\t')
                .append(tags).append('\t').append(noReturn ? 1 : 0).append('\t')
                .append(proto64).append('\t').append(ps64).append('\n');
            rows++;
        }
        Path path = Paths.get(out);
        Files.write(path, sb.toString().getBytes(StandardCharsets.UTF_8));
        println("EXPORT rows=" + rows + " bytes=" + sb.length() + " -> " + path);
    }
}
