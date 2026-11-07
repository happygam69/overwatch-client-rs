// Configure a quality analysis pass without function-start heuristics.
//
// Usage: analyzeHeadless ... -process <program>
//          -preScript ConfigureQualityAnalysis.java
//
// The clean map already has the executable's real .pdata functions. These
// analyzers are the ones that invent or repair function starts:
//   Function Start Search     - byte-pattern function discovery
//   Aggressive Instruction Finder - the same, more of it
//   Call-Fixup Installer      - call-fixup thunks and flow repair
//   Shared Return Calls       - shared-return thunk creation
//   Subroutine References     - call-target thunk creation
//   Decompiler Parameter ID   - bulk prototype inference
//   Non-Returning Functions - Discovered - flow repair rewrites bodies
//   Non-Returning Functions - Known      - applies textbook no-return
//                                          knowledge; 110 of the 179 flags it
//                                          left were provably wrong (the body
//                                          contains RET) and blinded 11,552
//                                          call sites. Leaving it on would
//                                          re-apply them after the cleanup.
//   PDB Universal                        - no symbol file ships with the
//                                          install, so it can only waste time
//   x86 Constant Reference Analyzer - decompiles every body and chases
//                                     invalid pointers outside the image
//                                     (observed looping); run it separately
// They stay off. The reference/data/string/demangler/stack analyzers stay on,
// and the analysis-bookmark options are turned off so the bookmark list stays
// useful.
import ghidra.app.script.GhidraScript;

public class ConfigureQualityAnalysis extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] options = {
            "Function Start Search",
            "Aggressive Instruction Finder",
            "Call-Fixup Installer",
            "Shared Return Calls",
            "Subroutine References",
            "Decompiler Parameter ID",
            "Non-Returning Functions - Discovered",
            "Non-Returning Functions - Discovered.Repair Flow Damage",
            "Non-Returning Functions - Known",
            "Non-Returning Functions - Known.Create Analysis Bookmarks",
            "PDB Universal",
            "x86 Constant Reference Analyzer",
            "Function ID.Create Analysis Bookmarks",
            "Non-Returning Functions - Discovered.Create Analysis Bookmarks",
            "Create Address Tables.Create Analysis Bookmarks",
            "Apply Data Archives.Create Analysis Bookmarks"
        };
        for (String option : options) {
            try {
                setAnalysisOption(currentProgram, option, "false");
                println("ANALYSIS disabled " + option);
            }
            catch (Exception e) {
                println("ANALYSIS could not disable " + option + ": " + e.getMessage());
            }
        }
    }
}
