// Merge each .pdata function's chained continuations into its body.
//
// The client splits a function into a hot part and one or more cold parts
// linked by `UNW_FLAG_CHAININFO`; a `.pdata` entry is one piece. Until this
// pass runs, a decompile of such a function ends at the hot part's last
// instruction, which jumps into code the function does not own - 25,184 bodies,
// 157 of them already named (`docs/ghidra-image-audit.md` section 8.2).
//
// This script resolves every chained entry to its transitive root (chains reach
// 8 deep in this image), takes the union of the root's pieces, and sets the
// root function's body to that union. Because the union is contiguous in every
// one of the 25,649 cases (measured from the image; `dev/corpus.py verify`
// re-checks the property on every run), the body stays a single range, so no
// function can end up fragmented by this pass.
//
// Args (whitespace-separated): `dry` (default) or `apply`.
//
//   MergeChainBodies.java dry      # report what would change, write nothing
//   MergeChainBodies.java apply    # remove inner objects, then widen bodies
//
// Expected on the current image (measured offline, 2026-11-17):
//   roots with a cold part          25,649
//   contiguous union                25,649
//   root has a function object      25,638   (11 have none; reported, skipped)
//   inner function objects removed  283      (the audit's AT_CHAIN list, exactly)
//   body bytes of those roots       2,572,571 -> 18,610,692  (+16,038,121)
//   `already`                       62 by the body-max comparison; the audit's
//                                   454 is bodies that already carry the
//                                   continuation by range, so `already` >= 62
//
// Proof after `apply`: `ImageAudit.java` must report
//   CHAIN_IMPACT in_body ~ 25,638 and body_flows_into_chain ~ 0
//   body_bytes 28.7 MB -> ~44.7 MB, exact + 454 unchanged
// and a `BodySnapshot.java` diff must show only the widened roots. Roll back
// with the folder copy recorded in `docs/ghidra-image-audit.md` section 11.
//
// Order: run this BEFORE deleting anything by hand - the function objects that
// sit at chained starts are exactly the inner objects this pass removes, and
// once the root owns their addresses they cannot come back.
//
// If Ghidra reports "ClassNotFoundException" after a previous script edit,
// restart Ghidra: its script class cache does not always pick up a changed file.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;

public class MergeChainBodies extends GhidraScript {

    private static long u32(byte[] b, int i) {
        return (long) (b[i] & 0xff)
            | (long) (b[i + 1] & 0xff) << 8
            | (long) (b[i + 2] & 0xff) << 16
            | (long) (b[i + 3] & 0xff) << 24;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String mode = args.length > 0 ? args[0].toLowerCase() : "dry";
        boolean apply = mode.equals("apply");
        boolean revert = mode.startsWith("revert");
        boolean revertApply = mode.equals("revert");
        long base = currentProgram.getImageBase().getOffset();
        Memory mem = currentProgram.getMemory();

        MemoryBlock pd = mem.getBlock(".pdata");
        if (pd == null) {
            println("FAIL: no .pdata block");
            return;
        }
        byte[] pb = new byte[(int) pd.getSize()];
        mem.getBytes(pd.getStart(), pb);

        // entry begin -> {begin, end, unwindRva, chained}
        Map<Long, long[]> entries = new HashMap<Long, long[]>();
        int chained = 0;
        for (int i = 0; i + 12 <= pb.length; i += 12) {
            long beg = u32(pb, i), end = u32(pb, i + 4), unw = u32(pb, i + 8);
            if (beg == 0 || end <= beg) {
                continue;
            }
            int flags = 0;
            try {
                flags = (mem.getByte(toAddr(base + unw)) & 0xff) >> 3 & 0x1f;
            }
            catch (Exception e) {
                continue;
            }
            boolean isChained = (flags & 4) != 0;
            if (isChained) {
                chained++;
            }
            entries.put(base + beg, new long[] { base + beg, base + end, base + unw, flags & 4 });
        }

        // Resolve every chained entry to its transitive root by walking the
        // CHAININFO record in the root's UNWIND_INFO: the unwind-code array is
        // padded to a DWORD when CountOfCodes is odd.
        Map<Long, List<long[]>> pieces = new HashMap<Long, List<long[]>>();
        int unresolvable = 0;
        for (long[] e : entries.values()) {
            if ((e[3] & 4) == 0) {
                continue;
            }
            long cur = e[0];
            HashSet<Long> seen = new HashSet<Long>();
            while (true) {
                long[] c = entries.get(cur);
                if (c == null || (c[3] & 4) == 0) {
                    break;
                }
                if (!seen.add(cur)) {
                    unresolvable++;
                    break;
                }
                int count = 0;
                try {
                    count = mem.getByte(toAddr(c[2] + 2)) & 0xff;
                }
                catch (Exception ex) {
                    unresolvable++;
                    break;
                }
                int off = 4 + 2 * count + ((count & 1) != 0 ? 2 : 0);
                long pbeg = 0;
                try {
                    pbeg = base + (mem.getInt(toAddr(c[2] + off)) & 0xffffffffL);
                }
                catch (Exception ex) {
                    unresolvable++;
                    break;
                }
                if (!entries.containsKey(pbeg)) {
                    unresolvable++;
                    break;
                }
                cur = pbeg;
            }
            List<long[]> list = pieces.get(cur);
            if (list == null) {
                list = new ArrayList<long[]>();
                pieces.put(cur, list);
            }
            list.add(e);
        }

        // `trace <addr>`: print one root's pieces and the first steps of the seam
        // walk, so a wrong verdict can be explained instead of guessed at.
        if (mode.equals("trace") && args.length > 1) {
            long want = Long.decode(args[1]);
            List<long[]> list = pieces.get(want);
            println(String.format("TRACE root=%x pieces=%d extent=%x..%x", want,
                list == null ? 0 : list.size() + 1, entries.get(want)[0], entries.get(want)[1]));
            if (list == null) {
                return;
            }
            List<long[]> sorted = new ArrayList<long[]>(list);
            sorted.add(entries.get(want));
            sorted.sort((a, b) -> Long.compare(a[0], b[0]));
            AddressSet union = new AddressSet();
            AddressSet frags = new AddressSet();
            for (long[] q : sorted) {
                union.add(toAddr(q[0]), toAddr(q[1] - 1));
                if (q[0] != want) {
                    frags.add(toAddr(q[0]), toAddr(q[1] - 1));
                }
            }
            println("TRACE fragments=" + frags);
            Function root = currentProgram.getFunctionManager().getFunctionAt(toAddr(want));
            println("TRACE body=" + root.getBody().getNumAddresses() + "B inUnion="
                + union.getNumAddresses() + "B");
            ArrayDeque<Address> work = new ArrayDeque<Address>();
            HashSet<Long> seen = new HashSet<Long>();
            work.add(root.getEntryPoint());
            int steps = 0;
            while (!work.isEmpty() && steps < 40) {
                Address a = work.poll();
                if (!seen.add(a.getOffset())) {
                    continue;
                }
                Instruction ins = currentProgram.getListing().getInstructionContaining(a);
                println(String.format("TRACE %x inUnion=%s inFrags=%s %s", a.getOffset(),
                    union.contains(a), frags.contains(a), ins == null ? "<undefined>" : ins.toString()));
                steps++;
                if (ins == null) {
                    continue;
                }
                Address ft = ins.getFallThrough();
                if (ft != null) {
                    work.add(ft);
                }
                for (Address t : ins.getFlows()) {
                    work.add(t);
                }
            }
            return;
        }

        FunctionManager fm = currentProgram.getFunctionManager();
        int roots = 0, contiguous = 0, notContiguous = 0, noFunction = 0;
        int already = 0, widened = 0, removed = 0, failed = 0;
        int outerOverlap = 0, outerPrinted = 0;
        int noSeam = 0, seamPrinted = 0, reverted = 0, revertSkipped = 0;
        StringBuilder noSeamList = new StringBuilder();
        long bytesBefore = 0, bytesAfter = 0;
        int printed = 0;

        int tx = currentProgram.startTransaction("Merge chained function bodies");
        boolean commit = false;
        try {
            for (Map.Entry<Long, List<long[]>> pair : pieces.entrySet()) {
                long rootBegin = pair.getKey();
                List<long[]> list = pair.getValue();
                roots++;

                List<long[]> sorted = new ArrayList<long[]>(list);
                sorted.add(entries.get(rootBegin));
                sorted.sort((a, b) -> Long.compare(a[0], b[0]));

                boolean ok = true;
                AddressSet union = new AddressSet();
                for (int i = 0; i < sorted.size(); i++) {
                    long[] p = sorted.get(i);
                    union.add(toAddr(p[0]), toAddr(p[1] - 1));
                    if (i > 0 && sorted.get(i - 1)[1] != p[0]) {
                        ok = false;
                    }
                }
                if (!ok) {
                    notContiguous++;
                    println(String.format("SKIP %x: pieces are not contiguous", rootBegin));
                    continue;
                }
                contiguous++;

                Function root = fm.getFunctionAt(toAddr(rootBegin));
                if (root == null) {
                    noFunction++;
                    continue;
                }
                bytesBefore += root.getBody().getNumAddresses();
                bytesAfter += union.getNumAddresses();

                AddressSet extent = new AddressSet(toAddr(entries.get(rootBegin)[0]),
                    toAddr(entries.get(rootBegin)[1] - 1));
                if (revert) {
                    // Only a body that was widened past its real extent, and whose
                    // fragments the entry cannot reach, goes back.
                    if (root.getBody().getNumAddresses() <= extent.getNumAddresses()) {
                        revertSkipped++;
                        continue;
                    }
                    if (seamStarts(root, union, sorted) > 0) {
                        revertSkipped++;
                        continue;
                    }
                    if (!revertApply) {
                        noSeamList.append(String.format("%x %d %d%n", rootBegin,
                            root.getBody().getNumAddresses(), extent.getNumAddresses()));
                        if (reverted < 10) {
                            println(String.format("WOULD-REVERT %x: %d B -> %d B (no seam)",
                                rootBegin, root.getBody().getNumAddresses(),
                                extent.getNumAddresses()));
                        }
                        reverted++;
                        continue;
                    }
                    noSeamList.append(String.format("%x %d %d%n", rootBegin,
                        root.getBody().getNumAddresses(), extent.getNumAddresses()));
                    try {
                        root.setBody(extent);
                        reverted++;
                    }
                    catch (Exception ex) {
                        failed++;
                        println(String.format("FAIL-REVERT %x: %s", rootBegin, ex.getMessage()));
                    }
                    continue;
                }

                if (root.getBody().equals(union)) {
                    already++;
                    continue;
                }

                // A function object whose ENTRY lies inside the union is a
                // chained start or a stray fragment: the root is about to own
                // those addresses, so the object must go first (Ghidra cannot
                // let two bodies overlap). An object whose entry lies OUTSIDE
                // the union while its body reaches in is a different, real
                // function - widening over it would delete somebody else's
                // extent, so such a root is skipped and reported instead.
                List<Address> victims = new ArrayList<Address>();
                List<Address> outsiders = new ArrayList<Address>();
                Iterator<Function> over = fm.getFunctionsOverlapping(union);
                while (over.hasNext()) {
                    Function f = over.next();
                    Address entry = f.getEntryPoint();
                    if (entry.equals(toAddr(rootBegin))) {
                        continue;
                    }
                    if (union.contains(entry)) {
                        victims.add(entry);
                    }
                    else {
                        outsiders.add(entry);
                    }
                }
                if (!outsiders.isEmpty()) {
                    outerOverlap++;
                    if (outerPrinted < 10) {
                        println(String.format("OUTER %x: %d function(s) outside the union "
                            + "reach into it, e.g. %x", rootBegin, outsiders.size(),
                            outsiders.get(0).getOffset()));
                        outerPrinted++;
                    }
                    continue;
                }

                // The seam rule: a fragment is this function's code only if the
                // entry can reach it by the function's own control flow. A
                // fragment nobody jumps to may belong to a different function
                // that merely shares this prologue/shadow space (the ABI allows a
                // chained record to share unwind data), so widening would take
                // someone else's code. Those stay out and are counted as NO-SEAM.
                int seams = seamStarts(root, union, sorted);
                if (seams == 0) {
                    noSeam++;
                    if (seamPrinted < 10) {
                        println(String.format("NO-SEAM %x: %d fragment(s), none reachable from "
                            + "the entry; body stays %d B", rootBegin, sorted.size() - 1,
                            root.getBody().getNumAddresses()));
                        seamPrinted++;
                    }
                    continue;
                }

                if (!apply) {
                    if (printed < 20) {
                        println(String.format("WOULD %x: %d B -> %d B, remove %d object(s)",
                            rootBegin, root.getBody().getNumAddresses(), union.getNumAddresses(),
                            victims.size()));
                        printed++;
                    }
                    widened++;
                    removed += victims.size();
                    continue;
                }

                try {
                    for (Address victim : victims) {
                        fm.removeFunction(victim);
                    }
                    removed += victims.size();
                    root.setBody(union);
                    widened++;
                }
                catch (Exception ex) {
                    failed++;
                    println(String.format("FAIL %x: %s", rootBegin, ex.getMessage()));
                }
            }
            if (apply || revertApply) {
                commit = true;
            }
        }
        finally {
            currentProgram.endTransaction(tx, commit);
        }

        println(String.format(
            "MERGE mode=%s roots=%d chained_entries=%d unresolvable=%d contiguous=%d "
            + "not_contiguous=%d no_function=%d already=%d outer_overlap=%d no_seam=%d "
            + "%s=%d %s=%d victims_removed=%d failed=%d body_bytes=%d->%d",
            mode, roots, chained, unresolvable, contiguous,
            notContiguous, noFunction, already, outerOverlap, noSeam,
            apply ? "widened" : "would_widen", widened,
            revert ? "reverted" : "kept", revert ? reverted : revertSkipped,
            removed, failed, bytesBefore, bytesAfter));
        if (revert) {
            try (java.io.PrintWriter w = new java.io.PrintWriter(
                    new java.io.BufferedWriter(new java.io.FileWriter(
                        "C:/Temp/owaudit/seam_unreached.txt")))) {
                w.print(noSeamList);
            }
            println("no-seam roots written to C:/Temp/owaudit/seam_unreached.txt");
            println("reverted roots to their own .pdata extent; save the program to keep it.");
        }
        else if (!apply) {
            println("dry run: nothing written. Run with `apply`, then ImageAudit + BodySnapshot.");
        }
        else {
            println("applied in one transaction; save the program to keep it.");
        }
    }

    /**
     * 1 when the entry's own flow (fall-through, jump, conditional jump) enters
     * the bytes of any fragment, 0 otherwise. Calls are not followed: a callee is
     * a separate function. Entry is tested by *bytes*, not by fragment start,
     * because a hot part may jump into the middle of a fragment.
     */
    private int seamStarts(Function root, AddressSet union, List<long[]> sorted) {
        AddressSet frags = new AddressSet();
        for (long[] p : sorted) {
            if (p[0] != root.getEntryPoint().getOffset()) {
                frags.add(toAddr(p[0]), toAddr(p[1] - 1));
            }
        }
        if (frags.isEmpty()) {
            return 0;
        }
        ArrayDeque<Address> work = new ArrayDeque<Address>();
        HashSet<Long> seen = new HashSet<Long>();
        work.add(root.getEntryPoint());
        while (!work.isEmpty()) {
            Address a = work.poll();
            if (a == null || !union.contains(a)) {
                continue;
            }
            if (!seen.add(a.getOffset())) {
                continue;
            }
            if (frags.contains(a)) {
                return 1;
            }
            Instruction ins = currentProgram.getListing().getInstructionContaining(a);
            if (ins == null) {
                continue;
            }
            // A call returns to the next instruction, so its fall-through is the
            // function's own flow; its target is a callee, not this function.
            Address ft = ins.getFallThrough();
            if (ft != null && union.contains(ft)) {
                work.add(ft);
            }
            if (!ins.getFlowType().isCall()) {
                for (Address t : ins.getFlows()) {
                    if (t != null && union.contains(t)) {
                        work.add(t);
                    }
                }
            }
        }
        return 0;
    }
}