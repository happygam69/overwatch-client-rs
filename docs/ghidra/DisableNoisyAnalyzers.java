// Disable the analyzers that invent function starts.
//
// Usage: analyzeHeadless ... -process <program> -analysis
//          -preScript DisableNoisyAnalyzers.java
//
// The clean map already has the PE's real .pdata functions. Function Start
// Search and Aggressive Instruction Finder are byte-pattern heuristics and
// must not add more. Decompiler Parameter ID stays off for the bulk pass.
import ghidra.app.script.GhidraScript;

public class DisableNoisyAnalyzers extends GhidraScript {
    @Override
    public void run() throws Exception {
        for (String option : new String[] {
            "Function Start Search",
            "Aggressive Instruction Finder",
            "Decompiler Parameter ID"
        }) {
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
