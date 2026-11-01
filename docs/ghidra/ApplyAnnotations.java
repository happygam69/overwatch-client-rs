// Apply the live project's function annotations to a clean project.
//
// Input is the TSV written by the export pass:
//   address  name  name_source  plate_b64  tags  no_return  prototype_b64  params_b64
//
// Names are applied only when name_source is USER. A named function that the
// clean .pdata map does not contain is created in a small window; if that
// window overlaps a real function the row is reported and skipped. Plates,
// tags, no-return flags, user prototypes and named parameters are applied by
// address.
//
// Usage:
//   analyzeHeadless ... -process <program> -noanalysis
//     -postScript ApplyAnnotations.java C:/Temp/opencode/ow_annotations.tsv
//
// This is a migration tool, not evidence: the names/plates come from the live
// project, and the clean map is the target.
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.cmd.function.ApplyFunctionSignatureCmd;
import ghidra.app.script.GhidraScript;
import ghidra.app.services.DataTypeQueryService;
import ghidra.app.util.parser.FunctionSignatureParser;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.data.CategoryPath;
import ghidra.program.model.data.DataType;
import ghidra.program.model.data.DataTypeManager;
import ghidra.program.model.data.DataTypePath;
import ghidra.program.model.data.FunctionDefinitionDataType;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.FunctionTag;
import ghidra.program.model.listing.FunctionTagManager;
import ghidra.program.model.listing.Parameter;
import ghidra.program.model.symbol.SourceType;
import ghidra.util.task.TaskMonitor;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Base64;
import java.util.Collections;
import java.util.List;

public class ApplyAnnotations extends GhidraScript {
    /** Enough of DataTypeQueryService for FunctionSignatureParser to resolve
     *  the program's own structs (MoverBody, MoverComponent, ...). */
    private static class DtmQuery implements DataTypeQueryService {
        private final DataTypeManager dtm;

        DtmQuery(DataTypeManager dtm) {
            this.dtm = dtm;
        }

        @Override
        public List<DataType> getSortedDataTypeList() {
            return Collections.emptyList();
        }

        @Override
        public List<CategoryPath> getSortedCategoryPathList() {
            return Collections.emptyList();
        }

        @Override
        public DataType getDataType(String name) {
            DataType dt = dtm.getDataType(name);
            if (dt == null) {
                dt = dtm.getDataType("/" + name);
            }
            if (dt == null) {
                dt = dtm.getDataType(CategoryPath.ROOT, name);
            }
            return dt;
        }

        @Override
        public DataType promptForDataType(String name) {
            return getDataType(name);
        }

        @Override
        public List<DataType> findDataTypes(String name, TaskMonitor monitor) {
            return Collections.emptyList();
        }

        @Override
        public List<DataType> getDataTypesByPath(DataTypePath path) {
            return Collections.emptyList();
        }

        @Override
        public DataType getProgramDataTypeByPath(DataTypePath path) {
            return dtm.getDataType(path.getCategoryPath(), path.getDataTypeName());
        }
    }

    private static String decode(String b64) {
        if (b64 == null || b64.isEmpty()) {
            return "";
        }
        return new String(Base64.getDecoder().decode(b64), StandardCharsets.UTF_8);
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: ApplyAnnotations <annotations.tsv>");
            return;
        }
        Path path = Paths.get(args[0]);
        List<String> lines = Files.readAllLines(path, StandardCharsets.UTF_8);
        FunctionManager fm = currentProgram.getFunctionManager();
        FunctionTagManager tags = fm.getFunctionTagManager();
        DataTypeManager dtm = currentProgram.getDataTypeManager();
        FunctionSignatureParser parser =
            new FunctionSignatureParser(dtm, new DtmQuery(dtm));

        int rows = 0, found = 0, created = 0, skipped = 0;
        int names = 0, plates = 0, tagAdds = 0, noReturns = 0, protos = 0, params = 0;
        for (String line : lines) {
            if (line.isEmpty() || line.startsWith("address\t")) {
                continue;
            }
            String[] col = line.split("\t", -1);
            if (col.length < 8) {
                continue;
            }
            rows++;
            Address address;
            try {
                address = toAddr(Long.parseLong(col[0], 16));
            }
            catch (Exception e) {
                skipped++;
                continue;
            }
            String name = col[1];
            boolean user = "USER".equals(col[2]);
            String plate = decode(col[3]);
            String tagList = col[4];
            boolean noReturn = "1".equals(col[5]);
            String proto = decode(col[6]);
            String paramList = decode(col[7]);

            Function f = fm.getFunctionAt(address);
            if (f == null) {
                if (!user || name.isEmpty()) {
                    skipped++;
                    continue;
                }
                // A named leaf that the .pdata map does not carry. Disassemble
                // a small window and create the function; overlap is reported.
                AddressSet window = new AddressSet(address, address.add(63));
                try {
                    new DisassembleCommand(address, window, true)
                        .applyTo(currentProgram, monitor);
                    f = fm.createFunction(null, address, window, SourceType.ANALYSIS);
                }
                catch (Exception e) {
                    f = null;
                }
                if (f == null) {
                    skipped++;
                    println("SKIP " + address + " " + name + " (window overlaps)");
                    continue;
                }
                created++;
            }
            found++;

            if (user && !name.isEmpty()) {
                try {
                    f.setName(name, SourceType.USER_DEFINED);
                    names++;
                }
                catch (Exception e) {
                    println("NAME FAIL " + address + " " + name + ": " + e.getMessage());
                }
            }
            if (!plate.isEmpty()) {
                f.setComment(plate);
                plates++;
            }
            if (!tagList.isEmpty()) {
                for (String tagName : tagList.split(";")) {
                    if (tagName.isEmpty()) {
                        continue;
                    }
                    FunctionTag tag = tags.getFunctionTag(tagName);
                    if (tag == null) {
                        tag = tags.createFunctionTag(tagName, "");
                    }
                    if (tag != null) {
                        f.addTag(tag.getName());
                        tagAdds++;
                    }
                }
            }
            if (noReturn) {
                f.setNoReturn(true);
                noReturns++;
            }
            if (!proto.isEmpty()) {
                try {
                    FunctionDefinitionDataType sig = parser.parse(null, proto);
                    new ApplyFunctionSignatureCmd(address, sig, SourceType.USER_DEFINED)
                        .applyTo(currentProgram, monitor);
                    // The client is MSVC x64; Ghidra names that convention
                    // __fastcall. The C signature parser does not carry it.
                    try {
                        f.setCallingConvention("__fastcall");
                    }
                    catch (Exception ce) {
                        println("CONV FAIL " + address + " " + name + ": " + ce.getMessage());
                    }
                    protos++;
                }
                catch (Exception e) {
                    println("PROTO FAIL " + address + " " + name + ": " + e.getMessage());
                }
            }
            if (!paramList.isEmpty()) {
                for (String pair : paramList.split(";")) {
                    int colon = pair.indexOf(':');
                    if (colon <= 0) {
                        continue;
                    }
                    try {
                        int ordinal = Integer.parseInt(pair.substring(0, colon));
                        String pname = pair.substring(colon + 1);
                        Parameter p = f.getParameter(ordinal);
                        if (p != null) {
                            p.setName(pname, SourceType.USER_DEFINED);
                            params++;
                        }
                    }
                    catch (Exception e) {
                        // A parameter-count mismatch is not fatal.
                    }
                }
            }
        }
        println("APPLY rows=" + rows + " found=" + found + " created=" + created
            + " skipped=" + skipped + " names=" + names + " plates=" + plates
            + " tagAdds=" + tagAdds + " noReturns=" + noReturns + " protos=" + protos
            + " paramNames=" + params);
    }
}
