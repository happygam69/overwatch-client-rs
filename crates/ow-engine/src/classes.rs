//! Every subsystem constructor, and the vtables it installs.
//!
//! Written by `dev/eng_gen_classes.py` from `dev/engine_classes.json`, which
//! `dev/eng_class_profile.py` decodes from the image: for each of the 35
//! producers the subsystem schedule uses, the constructor's own body is swept
//! (`lea reg,[rip+X]` then `mov [this+off],reg` is a vtable store; a `call`
//! ends the register tracking) and every vtable it installs is listed in store
//! order at the object offset it lands on.
//!
//! Two things to read into the data:
//!
//! * **A repeated offset is a base-then-derived overwrite.** The largest class
//!   (`+0x2f0`, ctor `0x14072c560`) installs `0x142f8ac68` and then its own
//!   table at each of six sub-object offsets — six copies of one base, each
//!   specialised. The profile keeps both stores, so the ordering is visible.
//! * **`stub_base` means the shared stub is involved.** 31 of the 65 slots are
//!   constructed by `0x141c578c0` itself (a 28-byte stub: vtable
//!   `0x142f7d778`, cookie `0x7495174655b5d763`, return its argument); the
//!   others call it from their own body. Either way the class derives from the
//!   same abstract base.
//!
//! The 117 vtables these rows name are decoded separately; 44 of them are
//! exactly 15 code entries (bounded from both sides — another known table at
//! `+0x78`, or a non-code word at `+0x78`), which is what a 15-method interface
//! table looks like. A table with no neighbour and no non-code word inside 128
//! entries has **no read extent**; the count in `dev/engine_classes.json` is a
//! lower bound, not a length. See `docs/formats/engine.md`.
//!
//! Three rows are producers rather than constructors: the `+0x220` decoder
//! `0x1405437f0` and the two factories `0x140594090`/`0x1406cc990`. They are
//! here because the schedule's producer column names them, and the profile
//! shows they install no table in their own body (the decoder's 16 `mov`s are
//! its fold, not field stores).
//!
//! The class *names* are not here: none is read yet. `ctor` is the identity,
//! and `ow_engine::subsystems::SUBSYSTEMS` links a slot to its constructor.

/// One vtable store the constructor performs: `vtable` lands at `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Install {
    /// Object offset the store targets.
    pub offset: u16,
    /// Vtable address in the image.
    pub vtable: u64,
}

/// One producer (constructor, decoder or factory), as read from its body.
#[derive(Debug)]
pub struct Class {
    /// Constructor entry point (image address).
    pub ctor: u64,
    /// Bytes of body read.
    pub body: u16,
    /// True when the body had no `.pdata` range and was bounded by its own
    /// terminating instruction instead (the stub and the `+0x220` decoder are
    /// leaf functions that `.pdata` omits).
    pub leaf: bool,
    /// True when the constructor calls the shared stub `0x141c578c0`, or *is*
    /// it.
    pub stub_base: bool,
    /// Vtable stores, in the order the constructor performs them.
    pub installs: &'static [Install],
}

/// One row per distinct constructor in the schedule, ordered by address.
pub const CLASSES: &[Class] = &[
    Class {
        ctor: 0x1405437f0,
        body: 41,
        leaf: true,
        stub_base: false,
        installs: &[],
    },
    Class {
        ctor: 0x14055cc60,
        body: 323,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f80f48,
            },
            Install {
                offset: 16,
                vtable: 0x142f80fc0,
            },
        ],
    },
    Class {
        ctor: 0x140594090,
        body: 38,
        leaf: false,
        stub_base: false,
        installs: &[],
    },
    Class {
        ctor: 0x140598900,
        body: 83,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f84270,
        }],
    },
    Class {
        ctor: 0x1405bc5c0,
        body: 44,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f84920,
            },
            Install {
                offset: 16,
                vtable: 0x142f84998,
            },
        ],
    },
    Class {
        ctor: 0x1405cf4f0,
        body: 33,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f84c70,
        }],
    },
    Class {
        ctor: 0x1405dac00,
        body: 178,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f84dd0,
            },
            Install {
                offset: 16,
                vtable: 0x142f84e48,
            },
            Install {
                offset: 32,
                vtable: 0x142f84dc0,
            },
        ],
    },
    Class {
        ctor: 0x14060fd30,
        body: 113,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f85dd8,
            },
            Install {
                offset: 16,
                vtable: 0x142f85e50,
            },
        ],
    },
    Class {
        ctor: 0x14061c0a0,
        body: 69,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f860f0,
            },
            Install {
                offset: 16,
                vtable: 0x142f86168,
            },
            Install {
                offset: 32,
                vtable: 0x142f86168,
            },
            Install {
                offset: 48,
                vtable: 0x142f86168,
            },
        ],
    },
    Class {
        ctor: 0x14061c4c0,
        body: 83,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f86178,
            },
            Install {
                offset: 16,
                vtable: 0x142f861f0,
            },
        ],
    },
    Class {
        ctor: 0x14061d1e0,
        body: 44,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f86208,
            },
            Install {
                offset: 16,
                vtable: 0x142f86280,
            },
        ],
    },
    Class {
        ctor: 0x14064f080,
        body: 135,
        leaf: false,
        stub_base: false,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f87588,
            },
            Install {
                offset: 16,
                vtable: 0x142f87750,
            },
            Install {
                offset: 1896,
                vtable: 0x142f87760,
            },
        ],
    },
    Class {
        ctor: 0x140651630,
        body: 85,
        leaf: false,
        stub_base: false,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f87810,
            },
            Install {
                offset: 16,
                vtable: 0x142f87948,
            },
            Install {
                offset: 736,
                vtable: 0x142f87958,
            },
        ],
    },
    Class {
        ctor: 0x140657b40,
        body: 133,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f87ba8,
        }],
    },
    Class {
        ctor: 0x140662760,
        body: 459,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f88188,
            },
            Install {
                offset: 16,
                vtable: 0x142f88200,
            },
        ],
    },
    Class {
        ctor: 0x14066b100,
        body: 663,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f882f0,
            },
            Install {
                offset: 16,
                vtable: 0x142f88368,
            },
        ],
    },
    Class {
        ctor: 0x1406735c0,
        body: 58,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f88700,
            },
            Install {
                offset: 16,
                vtable: 0x142f88700,
            },
            Install {
                offset: 24,
                vtable: 0x142f88700,
            },
            Install {
                offset: 32,
                vtable: 0x142f88700,
            },
            Install {
                offset: 40,
                vtable: 0x142f88700,
            },
            Install {
                offset: 56,
                vtable: 0x142f88700,
            },
        ],
    },
    Class {
        ctor: 0x1406928b0,
        body: 47,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f88fa0,
        }],
    },
    Class {
        ctor: 0x1406a4de0,
        body: 162,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f89078,
            },
            Install {
                offset: 16,
                vtable: 0x142f890f0,
            },
            Install {
                offset: 24,
                vtable: 0x142f89068,
            },
        ],
    },
    Class {
        ctor: 0x1406b4570,
        body: 63,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f89468,
        }],
    },
    Class {
        ctor: 0x1406b5000,
        body: 44,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f894e0,
            },
            Install {
                offset: 16,
                vtable: 0x142f89558,
            },
        ],
    },
    Class {
        ctor: 0x1406b58d0,
        body: 41,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f89570,
        }],
    },
    Class {
        ctor: 0x1406c0580,
        body: 104,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f89990,
            },
            Install {
                offset: 16,
                vtable: 0x142f89a08,
            },
        ],
    },
    Class {
        ctor: 0x1406c5380,
        body: 33,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f89ec8,
        }],
    },
    Class {
        ctor: 0x1406c6d10,
        body: 44,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f89fb8,
            },
            Install {
                offset: 16,
                vtable: 0x142f8a030,
            },
        ],
    },
    Class {
        ctor: 0x1406cc990,
        body: 38,
        leaf: false,
        stub_base: false,
        installs: &[],
    },
    Class {
        ctor: 0x140723890,
        body: 217,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 16,
                vtable: 0x142f8ab78,
            },
            Install {
                offset: 96,
                vtable: 0x142f8a710,
            },
            Install {
                offset: 0,
                vtable: 0x142f8a728,
            },
            Install {
                offset: 24,
                vtable: 0x142f7d758,
            },
            Install {
                offset: 0,
                vtable: 0x142f8aa28,
            },
            Install {
                offset: 24,
                vtable: 0x142f8aaa0,
            },
            Install {
                offset: 32,
                vtable: 0x142f8aac0,
            },
        ],
    },
    Class {
        ctor: 0x14072c560,
        body: 924,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f8ad80,
            },
            Install {
                offset: 16,
                vtable: 0x142f8ac68,
            },
            Install {
                offset: 16,
                vtable: 0x142f8ac90,
            },
            Install {
                offset: 144,
                vtable: 0x142f8ac68,
            },
            Install {
                offset: 144,
                vtable: 0x142f8acb8,
            },
            Install {
                offset: 256,
                vtable: 0x142f8ac68,
            },
            Install {
                offset: 256,
                vtable: 0x142f8ace0,
            },
            Install {
                offset: 344,
                vtable: 0x142f8ac68,
            },
            Install {
                offset: 344,
                vtable: 0x142f8ad08,
            },
            Install {
                offset: 424,
                vtable: 0x142f8ad08,
            },
            Install {
                offset: 464,
                vtable: 0x142f8ad08,
            },
            Install {
                offset: 504,
                vtable: 0x142f8ad08,
            },
            Install {
                offset: 656,
                vtable: 0x142f8ac68,
            },
            Install {
                offset: 656,
                vtable: 0x142f8ad30,
            },
            Install {
                offset: 736,
                vtable: 0x142f8ac68,
            },
            Install {
                offset: 736,
                vtable: 0x142f8ad58,
            },
        ],
    },
    Class {
        ctor: 0x14073fc00,
        body: 51,
        leaf: false,
        stub_base: false,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f8bc88,
            },
            Install {
                offset: 80,
                vtable: 0x142f8bc88,
            },
            Install {
                offset: 88,
                vtable: 0x142f8bc88,
            },
            Install {
                offset: 96,
                vtable: 0x142f8bc88,
            },
            Install {
                offset: 104,
                vtable: 0x142f8bc88,
            },
        ],
    },
    Class {
        ctor: 0x140741560,
        body: 33,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f8be90,
        }],
    },
    Class {
        ctor: 0x1407493c0,
        body: 33,
        leaf: false,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f8c088,
        }],
    },
    Class {
        ctor: 0x14074a300,
        body: 164,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f8c100,
            },
            Install {
                offset: 16,
                vtable: 0x142f8c190,
            },
            Install {
                offset: 0,
                vtable: 0x142f8c388,
            },
            Install {
                offset: 16,
                vtable: 0x142f8c190,
            },
            Install {
                offset: 32,
                vtable: 0x142f8c418,
            },
            Install {
                offset: 64,
                vtable: 0x142f8c418,
            },
            Install {
                offset: 168,
                vtable: 0x142f8c418,
            },
        ],
    },
    Class {
        ctor: 0x14074c5a0,
        body: 66,
        leaf: false,
        stub_base: false,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x142f8c508,
            },
            Install {
                offset: 16,
                vtable: 0x142f8c5c0,
            },
            Install {
                offset: 88,
                vtable: 0x142f7d8d0,
            },
            Install {
                offset: 96,
                vtable: 0x142f8c640,
            },
        ],
    },
    Class {
        ctor: 0x140bdf740,
        body: 51,
        leaf: false,
        stub_base: true,
        installs: &[
            Install {
                offset: 0,
                vtable: 0x1430a2910,
            },
            Install {
                offset: 16,
                vtable: 0x1430a2910,
            },
            Install {
                offset: 24,
                vtable: 0x1430a2910,
            },
            Install {
                offset: 32,
                vtable: 0x1430a2910,
            },
            Install {
                offset: 40,
                vtable: 0x1430a2910,
            },
        ],
    },
    Class {
        ctor: 0x141c578c0,
        body: 28,
        leaf: true,
        stub_base: true,
        installs: &[Install {
            offset: 0,
            vtable: 0x142f7d778,
        }],
    },
];

/// The class whose constructor is `ctor`, if the schedule uses one.
pub fn class_of(ctor: u64) -> Option<&'static Class> {
    CLASSES.iter().find(|c| c.ctor == ctor)
}

/// Every vtable any class installs, in class order (duplicates kept: the same
/// base is installed for several sub-objects).
pub fn installed_vtables() -> impl Iterator<Item = (u64, u16, u64)> {
    CLASSES
        .iter()
        .flat_map(|c| c.installs.iter().map(move |i| (c.ctor, i.offset, i.vtable)))
}

/// The vtable a class installs last at `offset` — what the object holds once
/// the constructor returns.
pub fn effective_vtable(ctor: u64, offset: u16) -> Option<u64> {
    let c = class_of(ctor)?;
    c.installs
        .iter()
        .filter(|i| i.offset == offset)
        .map(|i| i.vtable)
        .next_back()
}
