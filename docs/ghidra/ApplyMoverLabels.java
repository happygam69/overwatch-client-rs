// Name and label the mover code and data that the 2026-10-25 research touched.
//
// Names are only for functions whose role is settled. The two candidates whose
// role is not settled (the component field applier and the movement/state
// dispatcher fragment) get comments, not names, so the decompiler does not
// claim more than the docs do.
//
// Run headless against the project (Ghidra must be closed):
//   analyzeHeadless.bat E:\overwatch-client-rs\dev\ghidra_run owmod \
//     -process Overwatch.exe -noanalysis \
//     -scriptPath docs\ghidra -postScript ApplyMoverLabels.java
//
// Or run it from the GUI's Script Manager. Save the program afterwards.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;

public class ApplyMoverLabels extends GhidraScript {
    private void name(long address, String name) {
        Function f = getFunctionAt(toAddr(address));
        if (f == null) {
            println("MISSING function " + Long.toHexString(address) + " " + name);
            return;
        }
        try {
            f.setName(name, SourceType.USER_DEFINED);
            println("named " + name + " @ " + Long.toHexString(address));
        } catch (Exception e) {
            println("FAIL name " + name + ": " + e);
        }
    }

    private void comment(long address, String text) {
        Address a = toAddr(address);
        try {
            setPlateComment(a, text);
            println("comment " + Long.toHexString(address));
        } catch (Exception e) {
            println("FAIL comment " + Long.toHexString(address) + ": " + e);
        }
    }

    private void label(long address, String name, String text) {
        Address a = toAddr(address);
        try {
            createLabel(a, name, true, SourceType.USER_DEFINED);
            setEOLComment(a, text);
            println("label " + name + " @ " + Long.toHexString(address));
        } catch (Exception e) {
            println("FAIL label " + name + ": " + e);
        }
    }

    @Override
    public void run() throws Exception {
        // Settled roles: safe to name.
        name(0x140CB74A0L, "BuildMoverPhysicsCommand");
        name(0x140CC03D0L, "SubmitMoverPhysicsCommand");
        name(0x142CBCEE0L, "RunCharacterControllerSweep");
        name(0x142CBD1B0L, "SetCharacterControllerVelocity");
        name(0x142CD5370L, "ApplyCharacterControllerMove");
        name(0x142CD4BC0L, "MoveCharacterController");
        name(0x142CB9030L, "DispatchCharacterShapeQuery");
        name(0x142CF5770L, "SubmitCharacterControllerCommand");
        name(0x140CA4680L, "CopyMoverComponentFields");

        // Conclusions that belong next to the code.
        comment(0x14017A680L,
            "Registers STUCharacterMoverComponent: type hash 0x6EFDBD73, "
            + "component id 0x26, size 0x130. The default provider is at 0x143cf6990, "
            + "the descriptor at 0x143cf6a30.");
        comment(0x140CA37D0L,
            "ConstructMoverBase. The state-modifier table is an inline vector: "
            + "pointer at body+0x870 -> body+0x880, count at +0x878 (zero here), "
            + "capacity at +0x87c. Records are 0x38 bytes and runtime-populated.");
        comment(0x140CA7F30L,
            "MoverAdvance: position integrator reached through the base vtable "
            + "0x1431101e8 slot 15. It reads +0xc0..0xcc, which is a static local "
            + "offset (constructor-initialized), not a per-frame velocity; the "
            + "per-frame motion is the wish vector +0x1140 and transform +0x1a90.");
        comment(0x1407FFC50L,
            "MoverInvoker: calls MoverAdvance, stores the new position at +0x1b80, "
            + "then dispatches on the contact normals in the step state (+0x200).");
        comment(0x14068B000L,
            "Per-frame mover driver inside the .pdata range 0x14068ae00..0x14068b333. "
            + "Ghidra truncated the function at 0x14068ae00; use ResizePdataFunction.java "
            + "on that range before decompiling.");
        comment(0x140C8F4A2L,
            "Candidate component field applier: writes the runtime slots at "
            + "+0xac..+0xc8 (including the component's +0xc0 crouch slot). Whether it "
            + "is the mover-specific applier or a generic component applier is not "
            + "settled, so this function is not named.");
        comment(0x1407875E0L,
            "Movement/state dispatcher fragment. The bytes are plaintext and match "
            + "the warm images; Ghidra's body overlaps the next function, so this is "
            + "a boundary/analysis problem, not encrypted code. Role not settled.");

        // Vtables and static data used by the mover path.
        label(0x1431101E8L, "MoverBodyVtable",
            "Base vtable of the 0x1b70-byte mover body; MoverAdvance is slot 15 "
            + "(0x143110260).");
        label(0x1431101D8L, "MoverBodyVtableRegistration",
            "Derived/registration table for the mover body; repeats MoverAdvance at "
            + "slot 17 and ConstructMoverBody at slot 6.");
        label(0x142FDBF40L, "MoverBodyInterfaceVtable",
            "Character-body interface vtable; MoverInvoker is slot 9.");
        label(0x143110068L, "MoverComponentVtable0",
            "First interface vtable installed by ConstructMoverComponent "
            + "(component +0).");
        label(0x14310FF38L, "MoverComponentVtable1",
            "Second interface vtable installed by ConstructMoverComponent "
            + "(component +8).");
        label(0x14310FFD0L, "MoverComponentVtable2",
            "Third interface vtable installed by ConstructMoverComponent "
            + "(component +0x48).");
        label(0x143CF6A30L, "MoverComponentDescriptor",
            "Component type descriptor filled by RegisterMoverComponent: size 0x130, "
            + "52 fields, hash 0x6EFDBD73.");
        label(0x143CF6990L, "MoverComponentFieldTable",
            "Default provider / field table passed to RegisterMoverComponent.");
        label(0x143110260L, "MoverAdvanceSlot",
            "Base-vtable slot 15: MoverAdvance (0x140ca7f30).");

        println("done");
    }
}
