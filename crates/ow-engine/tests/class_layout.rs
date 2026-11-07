//! The subsystem classes: 35 constructors and the vtables they install.
//!
//! `ow_engine::classes::CLASSES` is written by `dev/eng_gen_classes.py` from
//! `dev/engine_classes.json`, which `dev/eng_class_profile.py` decodes from the
//! image. These tests pin what the data has to satisfy to be usable: every
//! schedule producer is a class, the shared stub is one class serving 31 slots,
//! installs land inside the allocation they belong to, the six-sub-object class
//! keeps its base-then-derived store order, and every vtable address is in the
//! tables' own region.

use ow_engine::classes::{class_of, effective_vtable, installed_vtables, CLASSES};
use ow_engine::subsystems::{members, subsystem, Source, STUB_CTOR, SUBSYSTEMS};

/// The two `.rdata` regions the vtables live in (`0x142f7...` and `0x1430a...`);
/// the second is where `+0x308`'s table sits, so the range is not one window.
const VTABLE_LO: u64 = 0x1_42f0_0000;
const VTABLE_HI: u64 = 0x1_430b_0000;

#[test]
fn there_are_thirty_five_constructors_and_ninety_six_installs() {
    assert_eq!(CLASSES.len(), 35);
    assert_eq!(installed_vtables().count(), 96);

    // 70 distinct tables from 96 stores: the same base is installed once per
    // sub-object (the six-sub-object class alone installs 0x142f8ac68 six
    // times).
    let mut class_vt: Vec<u64> = installed_vtables().map(|(_, _, v)| v).collect();
    class_vt.sort_unstable();
    class_vt.dedup();
    assert_eq!(class_vt.len(), 70);

    // The schedule block's own stores are a **disjoint** set of 47: a class
    // either installs its own `+0x00` table in its body (70 tables, no block
    // store) or only calls the shared base and the block writes the derived
    // table (47 tables). 70 + 47 = the 117 in `dev/engine_classes.json`.
    let mut block_vt: Vec<u64> = SUBSYSTEMS
        .iter()
        .flat_map(|s| std::iter::once(s.vtable).chain(s.extra.iter().map(|(_, v)| *v)))
        .filter(|v| *v != 0)
        .collect();
    block_vt.sort_unstable();
    block_vt.dedup();
    assert_eq!(block_vt.len(), 47);
    assert!(
        class_vt.iter().all(|v| !block_vt.contains(v)),
        "a table cannot be both class-installed and block-stored"
    );
}

#[test]
fn every_schedule_producer_is_a_class() {
    for s in members() {
        assert!(
            class_of(s.producer).is_some(),
            "slot {:#x}: producer {:#x} has no class",
            s.slot,
            s.producer
        );
    }
    assert_eq!(CLASSES.len(), 35);
}

#[test]
fn the_shared_stub_is_a_leaf_class_serving_thirty_one_slots() {
    let stub = class_of(STUB_CTOR).expect("stub");
    // No `.pdata` range: the body ends at its own `ret`.
    assert!(stub.leaf);
    assert_eq!(stub.body, 0x1c);
    assert!(stub.stub_base);
    assert_eq!(
        stub.installs,
        &[ow_engine::classes::Install {
            offset: 0,
            vtable: 0x142f7d778
        }]
    );

    let n = members().filter(|s| s.producer == STUB_CTOR).count();
    assert_eq!(n, 31);
}

#[test]
fn an_install_lands_inside_the_allocation_of_a_single_slot_class() {
    for s in members() {
        let Source::Alloc { size } = s.source else {
            continue;
        };
        // The stub's class installs at +0x00 and the slots that share it are
        // other classes' objects; only single-slot classes have one allocation.
        if s.producer == STUB_CTOR {
            continue;
        }
        let c = class_of(s.producer).expect("class");
        for i in c.installs {
            let end = i.offset as u64 + 8;
            assert!(
                end <= size as u64,
                "ctor {:#x} installs +{:#x} ({:#x}) past the {:#x}-byte allocation",
                c.ctor,
                i.offset,
                i.vtable,
                size
            );
        }
    }
}

#[test]
fn the_six_subobject_class_keeps_base_then_derived_order() {
    // `+0x2f0`: six sub-objects, each gets the shared base vtable and is then
    // specialised at the same offset. Ghidra's decompilation of `0x14072c560`
    // shows exactly this shape; keeping both stores is what makes it visible.
    let c = class_of(0x14072c560).expect("+0x2f0 ctor");
    assert!(c.stub_base);
    assert_eq!(c.installs.len(), 16);
    let shape: Vec<(u16, u64)> = c.installs.iter().map(|i| (i.offset, i.vtable)).collect();
    assert_eq!(
        shape,
        vec![
            (0x000, 0x142f8ad80),
            (0x010, 0x142f8ac68),
            (0x010, 0x142f8ac90),
            (0x090, 0x142f8ac68),
            (0x090, 0x142f8acb8),
            (0x100, 0x142f8ac68),
            (0x100, 0x142f8ace0),
            (0x158, 0x142f8ac68),
            (0x158, 0x142f8ad08),
            (0x1a8, 0x142f8ad08),
            (0x1d0, 0x142f8ad08),
            (0x1f8, 0x142f8ad08),
            (0x290, 0x142f8ac68),
            (0x290, 0x142f8ad30),
            (0x2e0, 0x142f8ac68),
            (0x2e0, 0x142f8ad58),
        ]
    );

    // What the object holds after the constructor: the derived table.
    assert_eq!(effective_vtable(0x14072c560, 0x010), Some(0x142f8ac90));
    assert_eq!(effective_vtable(0x14072c560, 0x1a8), Some(0x142f8ad08));
    // A slot the block stores but the constructor does not re-store.
    assert_eq!(effective_vtable(0x14072c560, 0x0f0), None);
}

#[test]
fn every_installed_vtable_is_aligned_and_inside_the_table_region() {
    for (ctor, offset, vt) in installed_vtables() {
        let v = vt;
        assert_eq!(v % 8, 0, "ctor {ctor:#x}: vtable {vt:#x} misaligned");
        assert!(
            (VTABLE_LO..VTABLE_HI).contains(&v),
            "ctor {ctor:#x}: vtable {vt:#x} at +{offset:#x} outside the table region"
        );
    }
}

#[test]
fn a_class_that_stores_no_vtable_still_calls_the_shared_base() {
    // The two factory slots (`+0x340`, `+0x358`) store no vtable in the block
    // and their producer installs none in its own body: the class identity is
    // the constructor alone.
    for slot in [0x340u32, 0x358] {
        let s = subsystem(slot).expect("slot");
        let c = class_of(s.producer).expect("class");
        assert!(c.installs.is_empty(), "slot {slot:#x}");
        assert_eq!(s.extra.len(), 0);
    }
}

#[test]
fn the_object_vtable_comes_from_the_block_or_from_the_constructor() {
    // Every slot's object carries a `+0x00` vtable, written in exactly one of
    // two places:
    //
    // * the **block** stores it — 32 slots, of which 31 are the shared-stub
    //   slots: there the stub writes `0x142f7d778` and the block overwrites it
    //   with the derived table immediately, so the stub's table never survives
    //   in a slot. The 32nd is `+0x308`, whose constructor installs
    //   `0x1430a2910` five times and whose block then re-points `+0x00`;
    // * the **constructor** installs it — 30 slots (the producer sets its own
    //   primary table and the block stores none);
    // * neither — 3 slots: `+0x220` (decoded pointer), `+0x340` and `+0x358`
    //   (factory returns). The object's table is not observable in this
    //   function for those; they are open, not empty.
    let mut from_block = 0;
    let mut from_ctor = 0;
    let mut neither = Vec::new();
    for s in members() {
        match (s.vtable != 0, effective_vtable(s.producer, 0).is_some()) {
            (true, _) => from_block += 1,
            (false, true) => from_ctor += 1,
            (false, false) => neither.push(s.slot),
        }
    }
    assert_eq!(from_block, 32);
    assert_eq!(from_ctor, 30);
    assert_eq!(neither, vec![0x340, 0x358, 0x220]); // store order

    // The stub's own table, for the record: written by `0x141c578c0` and
    // overwritten by every caller.
    assert_eq!(effective_vtable(STUB_CTOR, 0), Some(0x142f7d778));
    assert_eq!(SUBSYSTEMS.len(), 65);
}
