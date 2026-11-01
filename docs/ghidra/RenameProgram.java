// Rename the open program's domain file (the program name Ghidra lists).
//
// Usage:
//   analyzeHeadless ... -process <old-name> -noanalysis
//     -postScript RenameProgram.java Overwatch.exe
//
// Used when a clean import is created from a file named Overwatch_clean.exe
// but the function-name table (and the rest of the project) says Overwatch.exe.
import ghidra.app.script.GhidraScript;
import ghidra.framework.model.DomainFile;

public class RenameProgram extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: RenameProgram <new-name>");
            return;
        }
        DomainFile df = currentProgram.getDomainFile();
        String old = df.getName();
        DomainFile renamed = df.setName(args[0]);
        println("RENAMED " + old + " -> " + renamed.getName() + " (" + renamed.getPathname() + ")");
    }
}
