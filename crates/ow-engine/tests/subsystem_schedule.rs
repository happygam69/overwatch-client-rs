//! `ConstructEngineSubsystems` (`0x140554220`) as data, checked against itself.
//!
//! The rows in `ow_engine::subsystems::SUBSYSTEMS` are written by
//! `dev/eng_gen_subsystems.py` from `dev/engine_subsystems.verified.csv`,
//! which `dev/eng_verify_schedule.py` re-derives instruction by instruction
//! from the function's own bytes. These tests pin the structural facts that
//! make the table usable and catch a bad transcription: the window, the five
//! empty offsets, the store order, the counts that reconcile (80 allocations,
//! 84 registrations, 65 stores), the allocation-size histogram, the shared
//! stub, the four non-`Alloc` sources and the tail attach pass.

use std::collections::HashSet;

use ow_engine::subsystems::{
    is_member_slot, members, slot_index, slot_offset, subsystem, window, Source, Subsystem,
    ALLOC_CALLS, EMPTY_SLOTS, MEMBERS, OBJECT_SIZE, REGISTER_CALLS, REGISTER_ONLY, SLOT_BASE,
    SLOT_END, SLOT_STRIDE, SLOT_WINDOW, STUB_CTOR, SUBSYSTEMS, TAIL_ATTACH,
};
use ow_engine::world::World;

#[test]
fn the_window_is_seventy_slots_and_sixty_five_are_members() {
    assert_eq!(SLOT_BASE, 0x220);
    assert_eq!(SLOT_END, 0x448);
    assert_eq!(SLOT_STRIDE, 8);
    assert_eq!(SLOT_WINDOW, 70);
    assert_eq!(MEMBERS, 65);
    assert_eq!(SUBSYSTEMS.len(), MEMBERS);

    // The window ends before the object does (compile-time, it is all consts).
    const _: () = assert!(SLOT_END + SLOT_STRIDE <= OBJECT_SIZE);
}

#[test]
fn every_member_is_aligned_and_inside_the_window() {
    for s in members() {
        assert_eq!(s.slot % SLOT_STRIDE, 0, "slot {:#x}", s.slot);
        assert!(
            (SLOT_BASE..=SLOT_END).contains(&s.slot),
            "slot {:#x}",
            s.slot
        );
        assert!(s.slot + 4 <= OBJECT_SIZE);
    }
}

#[test]
fn no_slot_is_written_twice_and_the_five_empty_offsets_are_exactly_the_rest() {
    let mut seen = HashSet::new();
    for s in members() {
        assert!(seen.insert(s.slot), "slot {:#x} appears twice", s.slot);
    }

    let empty: Vec<u32> = window()
        .filter(|(_, s)| s.is_none())
        .map(|(slot, _)| slot)
        .collect();
    assert_eq!(empty, EMPTY_SLOTS.to_vec());
    assert_eq!(seen.len() + EMPTY_SLOTS.len(), SLOT_WINDOW);

    for slot in EMPTY_SLOTS {
        assert!(!is_member_slot(slot), "slot {:#x}", slot);
        assert!(subsystem(slot).is_none());
    }
}

#[test]
fn lookup_is_slot_order_agnostic() {
    for (i, s) in SUBSYSTEMS.iter().enumerate() {
        assert_eq!(slot_index(s.slot), Some(i));
        assert_eq!(slot_offset(i), s.slot);
        assert!(is_member_slot(s.slot));
        assert_eq!(subsystem(s.slot).map(|r| r.slot), Some(s.slot));
    }
    assert_eq!(slot_index(0x224), None);
    assert_eq!(slot_index(0x450), None);
}

#[test]
fn the_table_is_in_store_order() {
    // The schedule is emitted in the order the stores occur, which is not
    // slot order. First store: 0x1405542c9 -> +0x408 (Alloc 0x100, ctor
    // 0x140662760). Last store: 0x1405556d9 -> +0x438.
    assert_eq!(SUBSYSTEMS[0].slot, 0x408);
    assert_eq!(SUBSYSTEMS[0].store_site, 0x1_4055_42c9);
    assert_eq!(SUBSYSTEMS[SUBSYSTEMS.len() - 1].slot, 0x438);
    assert_eq!(SUBSYSTEMS[SUBSYSTEMS.len() - 1].store_site, 0x1_4055_56d9);

    let sites: Vec<u64> = members().map(|s| s.store_site).collect();
    let mut sorted = sites.clone();
    sorted.sort_unstable();
    assert_eq!(sites, sorted, "store sites must be in instruction order");
    assert_eq!(sites.iter().collect::<HashSet<_>>().len(), MEMBERS);
}

#[test]
fn the_counts_reconcile() {
    assert_eq!(ALLOC_CALLS, 80);
    assert_eq!(REGISTER_CALLS, 84);
    assert_eq!(REGISTER_ONLY, 20);
    // 65 stores = 61 inline allocations + 4 other sources.
    assert_eq!(members().filter(|s| s.allocation().is_some()).count(), 61);
    assert_eq!(
        members()
            .filter(|s| matches!(s.source, Source::Factory))
            .count(),
        2
    );
    assert_eq!(
        members()
            .filter(|s| matches!(s.source, Source::Decoded { .. }))
            .count(),
        1
    );
    assert_eq!(
        members()
            .filter(|s| matches!(s.source, Source::Adjustor { .. }))
            .count(),
        1
    );
    // 65 stores: 64 registrations whose object reaches a slot (the 0x340 and
    // 0x358 factories have one each) plus 0x220, whose decoded pointer is not
    // registered.
    let registrations: Vec<u64> = members()
        .filter_map(|s| (s.register_site != 0).then_some(s.register_site))
        .collect();
    assert_eq!(registrations.len(), 64);
    assert_eq!(registrations.iter().collect::<HashSet<_>>().len(), 64);
    let mut sorted = registrations.clone();
    sorted.sort_unstable();
    assert_eq!(registrations, sorted);
    assert_eq!(registrations[0], 0x1_4055_42d3);
    assert_eq!(*registrations.last().unwrap(), 0x1_4055_56e3);
}

#[test]
fn the_allocation_size_histogram_is_the_functions() {
    // 61 inline allocations, 19 distinct sizes. `+0x10` and `+0x18` are the
    // stub-built interface objects; the rest are one or two each.
    let mut hist: Vec<(u32, usize)> = Vec::new();
    for s in members() {
        if let Some(size) = s.allocation() {
            match hist.iter_mut().find(|(s, _)| *s == size) {
                Some((_, n)) => *n += 1,
                None => hist.push((size, 1)),
            }
        }
    }
    hist.sort_unstable();
    assert_eq!(
        hist,
        vec![
            (0x10, 23),
            (0x18, 16),
            (0x28, 1),
            (0x30, 2),
            (0x38, 1),
            (0x40, 2),
            (0x58, 2),
            (0x68, 2),
            (0x70, 1),
            (0x88, 2),
            (0x90, 1),
            (0xb0, 1),
            (0xc0, 1),
            (0xd0, 1),
            (0x100, 1),
            (0x1b8, 1),
            (0x2f8, 1),
            (0x390, 1),
            (0x7d0, 1),
        ]
    );
}

#[test]
fn thirty_one_slots_are_the_shared_stub() {
    let stubs: Vec<&Subsystem> = members().filter(|s| s.is_stub()).collect();
    assert_eq!(stubs.len(), 31);
    assert_eq!(STUB_CTOR, 0x1_41c5_78c0);
    // Every stub caller immediately overwrites the stub's vtable, so each of
    // these rows carries a primary vtable of its own.
    for s in stubs {
        assert_ne!(s.vtable, 0, "slot {:#x}", s.slot);
        assert_eq!(s.producer, STUB_CTOR);
    }
    // The one non-stub row that also overwrites its constructor's vtable.
    let overridden = subsystem(0x308).expect("slot 0x308");
    assert_eq!(overridden.producer, 0x1_40bd_f740);
    assert_eq!(overridden.vtable, 0x1_42f7_f298);
}

#[test]
fn the_four_slots_without_an_inline_allocation_are_read_as_such() {
    // 0x220: [world+0x330] + 0x60, decoded in place by 0x1405437f0 and then
    // folded by the same expression the constructor uses.
    let decoded = subsystem(0x220).expect("slot 0x220");
    assert_eq!(
        decoded.source,
        Source::Decoded {
            from_slot: 0x330,
            offset: 0x60
        }
    );
    assert_eq!(decoded.producer, 0x1_4054_37f0);
    assert_eq!(decoded.register_site, 0, "0x220 is never registered");
    assert_eq!(decoded.store_site, 0x1_4055_5372);

    // 0x340 / 0x358: factories that allocate inside themselves, one
    // registration each (0x140554ae3 / 0x140554af2).
    for (slot, producer, store, register) in [
        (0x340, 0x1_4059_4090, 0x1_4055_4ac6, 0x1_4055_4ae3),
        (0x358, 0x1_406c_c990, 0x1_4055_4adc, 0x1_4055_4af2),
    ] {
        let s = subsystem(slot).expect("factory slot");
        assert_eq!(s.source, Source::Factory);
        assert!(s.allocation().is_none());
        assert_eq!(s.producer, producer);
        assert_eq!(s.store_site, store);
        assert_eq!(s.register_site, register);
    }

    // 0x448: the object is 0x68 bytes and is registered *before* the adjustor
    // is stored, so this is the one row whose registration site precedes its
    // store.
    let adjustor = subsystem(0x448).expect("slot 0x448");
    assert_eq!(adjustor.source, Source::Adjustor { object_size: 0x68 });
    assert_eq!(adjustor.producer, 0x1_4072_3890);
    assert_eq!(adjustor.register_site, 0x1_4055_4b8f);
    assert_eq!(adjustor.store_site, 0x1_4055_4bba);
    assert!(adjustor.register_site < adjustor.store_site);
    assert!(members()
        .filter(|s| s.register_site != 0)
        .all(|s| s.slot == 0x448 || s.register_site > s.store_site));
}

#[test]
fn the_vtable_table_is_the_blocks() {
    // 32 rows store a primary vtable inline; all 32 values are distinct.
    let primaries: Vec<u64> = members()
        .filter(|s| s.vtable != 0)
        .map(|s| s.vtable)
        .collect();
    assert_eq!(primaries.len(), 32);
    assert_eq!(primaries.iter().collect::<HashSet<_>>().len(), 32);

    // 16 further vtable stores across the slots; 15 distinct values because
    // 0x3f8 and 0x420 share the secondary base 0x142f7e168.
    let mut extras = Vec::new();
    for s in members() {
        for &(off, vtable) in s.extra {
            extras.push((s.slot, off, vtable));
        }
    }
    assert_eq!(extras.len(), 16);
    let targets: HashSet<u64> = extras.iter().map(|&(_, _, v)| v).collect();
    assert_eq!(targets.len(), 15);

    let shared: Vec<_> = extras
        .iter()
        .filter(|&&(_, _, v)| v == 0x1_42f7_e168)
        .collect();
    assert_eq!(shared.len(), 2);
    assert_eq!(shared[0].0, 0x3f8);
    assert_eq!(shared[1].0, 0x420);

    // The widest block: slot 0x278 carries four secondary bases.
    let widest = subsystem(0x278).expect("slot 0x278");
    assert_eq!(
        widest.extra,
        &[
            (0x10, 0x1_42f7_e6c8),
            (0x38, 0x1_42f7_e6f8),
            (0x60, 0x1_42f7_e728),
            (0x88, 0x1_42f7_e740),
        ]
    );
    // A stub-built two-interface object.
    let two = subsystem(0x250).expect("slot 0x250");
    assert_eq!(two.vtable, 0x1_42f7_fc90);
    assert_eq!(two.extra, &[(0x10, 0x1_42f7_fd08)]);
}

#[test]
fn the_tail_attach_pass_is_the_twenty_one_in_call_order() {
    assert_eq!(TAIL_ATTACH.len(), 21);
    let sites: Vec<u64> = TAIL_ATTACH.iter().map(|t| t.site).collect();
    let mut sorted = sites.clone();
    sorted.sort_unstable();
    assert_eq!(sites, sorted, "tail attach is a straight-line pass");

    let slots: Vec<u32> = TAIL_ATTACH.iter().map(|t| t.slot).collect();
    assert_eq!(
        slots,
        vec![
            0x368, 0x288, 0x2e8, 0x2d8, 0x300, 0x2e0, 0x270, 0x238, 0x3b0, 0x250, 0x240, 0x290,
            0x248, 0x398, 0x3e0, 0x3e8, 0x3f0, 0x3f8, 0x400, 0x410, 0x268,
        ]
    );
    // Each one re-attaches an object this function stored a moment earlier.
    for t in TAIL_ATTACH {
        assert!(is_member_slot(t.slot), "slot {:#x}", t.slot);
    }
    // Only the last is guarded (cmp qword ptr [rsi+0xe0], 0).
    assert!(TAIL_ATTACH.iter().filter(|t| t.guarded).count() == 1);
    assert!(TAIL_ATTACH.last().unwrap().guarded);
    assert_eq!(TAIL_ATTACH.last().unwrap().slot, 0x268);
    assert_eq!(TAIL_ATTACH.first().unwrap().site, 0x1_4055_5704);
    assert_eq!(TAIL_ATTACH.last().unwrap().site, 0x1_4055_599f);
}

#[test]
fn world_slots_start_where_the_constructors_leave_them() {
    let world = World::new();
    // All but +0x220 are zero from the constructor's table; +0x220 holds the
    // encoded world id the subsystem pass overwrites.
    assert_eq!(
        world.subsystem_slot(0x220).map(|p| p as u64),
        Some(world.enc_world_id)
    );
    for s in members().filter(|s| s.slot != 0x220) {
        assert!(
            world.subsystem_slot(s.slot).is_some_and(|p| p.is_null()),
            "slot {:#x} not null",
            s.slot
        );
    }
}

#[test]
fn world_refuses_offsets_the_function_never_writes() {
    let mut world = World::new();
    for bad in [
        0x000, 0x218, 0x228, 0x230, 0x350, 0x418, 0x440, 0x450, 0x46c,
    ] {
        assert!(world.subsystem_slot_address(bad).is_none(), "slot {bad:#x}");
        assert!(world.subsystem_slot(bad).is_none(), "slot {bad:#x}");
        // SAFETY: null is always an acceptable slot value.
        assert!(!unsafe { world.set_subsystem_slot(bad, core::ptr::null_mut()) });
    }
}

#[test]
fn world_stores_and_reads_each_member_slot() {
    let mut world = World::new();
    let mut addresses = HashSet::new();
    for (i, s) in members().enumerate() {
        // The address of the slot is inside the object and unique per slot.
        let addr = world
            .subsystem_slot_address(s.slot)
            .expect("member slot address");
        assert!(
            addresses.insert(addr as usize),
            "slot {:#x} aliases",
            s.slot
        );

        let value = (0x1000 + i) as *mut u8;
        // SAFETY: the test never dereferences a slot value.
        assert!(unsafe { world.set_subsystem_slot(s.slot, value) });
        assert_eq!(world.subsystem_slot(s.slot), Some(value));
    }
    // The five empty offsets stayed zero through the same window.
    for slot in EMPTY_SLOTS {
        let p =
            unsafe { (world.subsystem_slot_address(0x220).unwrap()).add(slot as usize - 0x220) };
        let q = unsafe { core::ptr::read_unaligned(p.cast::<u64>()) };
        assert_eq!(q, 0, "empty slot {slot:#x} was written");
    }
}
