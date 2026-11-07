//! The client's 64-bit value transforms, operation for operation.
//!
//! The engine never stores a pointer, handle or counter as itself. Each stored
//! value is `transform(value)` where the transform is a short chain of
//! wrapping arithmetic, rotates and xors against 64-bit constants that differ
//! per field. Fields that are transformed in more than one place get a tiny
//! out-of-line helper of the shape
//!
//! ```text
//! mov rax, [rcx]              load the stored value
//! <op> rax, <imm64|imm8>      one to four operations
//! mov [rcx], rax              store it back
//! ret
//! ```
//!
//! and the same helper called from both sides is what makes the pair
//! recoverable: `0x1405437f0` and its inverse `0x1405439c0` are the same
//! transform with the operations reversed and each one replaced by its
//! inverse. `dev/codec_scan.py` enumerates the whole mechanical family from
//! the image, so a new site can be checked against it instead of guessed.
//!
//! Each constant below names the site it was read from. The operation order is
//! the execution order, so `apply` walks the slice forwards and `revert` walks
//! it backwards, inverting as it goes.

/// One step of a transform. The names are the instructions the client uses.
///
/// Shift counts are masked to six bits, as `shl`/`shr`/`rol`/`ror` do on a
/// 64-bit operand — the client never rotates by more than 63, but masking
/// keeps the model identical to the hardware rather than to an assumption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `add rax, imm`
    Add(u64),
    /// `sub rax, imm`
    Sub(u64),
    /// `xor rax, imm`
    Xor(u64),
    /// `ror rax, imm8`
    Ror(u32),
    /// `rol rax, imm8`
    Rol(u32),
    /// `shl rax, imm8`
    Shl(u32),
    /// `shr rax, imm8`
    Shr(u32),
    /// `neg rax`
    Neg,
    /// `not rax`
    Not,
}

impl Op {
    /// The step that undoes this one.
    #[must_use]
    pub const fn inverse(self) -> Self {
        match self {
            Op::Add(k) => Op::Sub(k),
            Op::Sub(k) => Op::Add(k),
            Op::Xor(k) => Op::Xor(k),
            Op::Ror(n) => Op::Rol(n),
            Op::Rol(n) => Op::Ror(n),
            Op::Shl(n) => Op::Shr(n),
            Op::Shr(n) => Op::Shl(n),
            Op::Neg | Op::Not => self,
        }
    }

    /// One step, exactly as the instruction behaves.
    #[must_use]
    pub fn forward(self, v: u64) -> u64 {
        match self {
            Op::Add(k) => v.wrapping_add(k),
            Op::Sub(k) => v.wrapping_sub(k),
            Op::Xor(k) => v ^ k,
            Op::Ror(n) => v.rotate_right(n & 63),
            Op::Rol(n) => v.rotate_left(n & 63),
            Op::Shl(n) => v << (n & 63),
            Op::Shr(n) => v >> (n & 63),
            Op::Neg => v.wrapping_neg(),
            Op::Not => !v,
        }
    }
}

/// A transform: the operations of one client helper, in execution order.
#[derive(Clone, Copy, Debug)]
pub struct Codec {
    ops: &'static [Op],
}

impl Codec {
    /// A transform made of `ops`, applied in order.
    #[must_use]
    pub const fn new(ops: &'static [Op]) -> Self {
        Self { ops }
    }

    /// The operations, in execution order.
    #[must_use]
    pub const fn ops(self) -> &'static [Op] {
        self.ops
    }

    /// Value as stored: the transform the client applies on the way in.
    #[must_use]
    pub fn apply(self, value: u64) -> u64 {
        self.ops.iter().fold(value, |v, op| op.forward(v))
    }

    /// Value as used: the inverse transform, which the client's paired helper
    /// performs on the way out.
    #[must_use]
    pub fn revert(self, stored: u64) -> u64 {
        self.ops
            .iter()
            .rev()
            .fold(stored, |v, op| op.inverse().forward(v))
    }
}

/// The transform on the world-interface array (`world+0x188`) and on the
/// pointer field at `world+0x058`.
///
/// Encode, inline in `RegisterWorldInterface` (`0x141c52d44`..`0x141c52d59`):
/// `xor 0x238e31e8cd062f58`, `ror 0x31`, `add 0x34b5f414245bf6ef`.
///
/// The encoded null — `apply(0)` — is `0x4daa5a973c0808b6`, which
/// `WorldBaseCtor` writes to `world+0x058` as a literal (`0x141c50a2c`
/// region, `param_1[0xb] = 0x4daa5a973c0808b6`): an empty pointer field, not a
/// zeroed one.
pub const INTERFACE_ARRAY: Codec = Codec::new(&[
    Op::Xor(0x238e31e8cd062f58),
    Op::Ror(0x31),
    Op::Add(0x34b5f414245bf6ef),
]);

/// The transform used by the helper at `0x140543a40`:
/// `sub 0x274e2f82e6194a03`, `xor 0x45ef9e20123f172f`.
///
/// `WorldBaseCtor` runs it over the constant `0x627dce6e1d5603a4` and xors the
/// result with `0xd674a580a443b55e` for `world+0x038`; its inverse is the
/// paired helper at `0x1405439c0`.
pub const SEED_38: Codec = Codec::new(&[Op::Sub(0x274e2f82e6194a03), Op::Xor(0x45ef9e20123f172f)]);

/// The transform used by the helper at `0x1405674e0`:
/// `xor 0xd6a9bd8714cc2178`, `sub 0x416ff37b1ad4aa2f`.
///
/// `WorldBaseCtor` runs it over zero and folds the result into `world+0x0b0`
/// (`+ 0xcf64407ada39d02e`, `xor 0xb2b338251ec2701`, `rol 0x17`); its inverse
/// is the paired helper the scanner lists as
/// `add:0x416ff37b1ad4aa2f xor:0xd6a9bd8714cc2178`.
pub const SEED_B0: Codec = Codec::new(&[Op::Xor(0xd6a9bd8714cc2178), Op::Sub(0x416ff37b1ad4aa2f)]);

/// The transform used by the helper at `0x1405437f0`:
/// `ror 15`, `xor 0x5a1b63795b8475ea`, `sub 0x4998612e34f258d3`, `ror 30`.
///
/// `WorldCtor` seeds it with zero to make the encoded world id at
/// `world+0x220`.
pub const SEED_A: Codec = Codec::new(&[
    Op::Ror(15),
    Op::Xor(0x5a1b63795b8475ea),
    Op::Sub(0x4998612e34f258d3),
    Op::Ror(30),
]);

/// The transform used by the helper at `0x1405838a0`:
/// `sub 0x574da7a34c106b16`, `ror 3`.
///
/// `WorldCtor` uses it twice for `world+0x460`: first on the constant
/// `0xf45f13eca6129815` xor `0x689f22ff8960a2c7` (0x140583bb2), then on the
/// address mix [`OBJ_460`] describes (0x140583c2b), which is the value the
/// field ends up holding. Its inverse is the paired helper `0x1405838d0`
/// (`rol 3`, `add 0x574da7a34c106b16`).
pub const SEED_B: Codec = Codec::new(&[Op::Sub(0x574da7a34c106b16), Op::Ror(3)]);

/// The transform `0x141c56fb0` uses for the back pointer it writes into a
/// registered subsystem: `xor 0xd6c0ac78d0e44481`, `add 0x9dd46acd84d192e2`.
///
/// In `0x141c56fb0` the engine pointer arrives in `rcx` and the store is
/// `mov [rdx+8], rax` — every registered subsystem holds its engine this way,
/// not as a plain pointer.
pub const BACK_PTR: Codec = Codec::new(&[Op::Xor(0xd6c0ac78d0e44481), Op::Add(0x9dd46acd84d192e2)]);

/// The transform `WorldCtor` applies to the address of the `0x1b8`-byte object
/// it builds and drops, before storing it at `world+0x460` (0x140583c2b):
/// `xor 0xad02ee81538f03d8`, `add 0x4bbcf58285b7b2ec`, `ror 0x1a`,
/// `add 0xa2b162ae7619971f`, then [`SEED_B`], then `xor 0x689f22ff8960a2c7`.
///
/// Its inverse is the same sequence reversed with each operation's inverse, so
/// a test can prove the field is an encoded address and get the address back
/// (`ror` becomes `rol`, `add` becomes `sub`).
pub const OBJ_460: Codec = Codec::new(&[
    Op::Xor(0xad02ee81538f03d8),
    Op::Add(0x4bbcf58285b7b2ec),
    Op::Ror(0x1a),
    Op::Add(0xa2b162ae7619971f),
    Op::Sub(0x574da7a34c106b16),
    Op::Ror(3),
    Op::Xor(0x689f22ff8960a2c7),
]);

#[cfg(test)]
mod tests {
    use super::*;

    /// Values worth carrying through every codec: the two ends of the range,
    /// a null pointer, a small pointer and a live-capture-shaped pointer.
    const SAMPLES: [u64; 6] = [
        0,
        1,
        0x1000,
        0x7ff6_0000_1000,
        0xffff_ffff_ffff_ffff,
        0x0123_4567_89ab_cdef,
    ];

    #[test]
    fn every_codec_round_trips_every_sample() {
        for codec in [
            INTERFACE_ARRAY,
            SEED_38,
            SEED_B0,
            SEED_A,
            SEED_B,
            BACK_PTR,
            OBJ_460,
        ] {
            for v in SAMPLES {
                assert_eq!(
                    codec.revert(codec.apply(v)),
                    v,
                    "codec {:?} did not round trip {v:#018x}",
                    codec.ops()
                );
            }
        }
    }

    /// `apply(0)` on the interface codec is the literal the base constructor
    /// stores at `world+0x058`, so this pins the codec against the image.
    #[test]
    fn interface_codec_encoded_null_is_the_constructor_constant() {
        assert_eq!(INTERFACE_ARRAY.apply(0), 0x4daa_5a97_3c08_08b6);
    }

    #[test]
    fn interface_codec_golden_values() {
        // Computed from the three instructions at 0x141c52d44..0x141c52d59 by
        // hand and re-derived independently in `dev/codec_scan.py`'s terms.
        assert_eq!(INTERFACE_ARRAY.apply(0x1000), 0x4daa_5a97_4408_08b6);
        assert_eq!(
            INTERFACE_ARRAY.apply(0x7ff6_0000_1000),
            0x5bc5_5a97_4408_08b6
        );
        assert_eq!(INTERFACE_ARRAY.apply(u64::MAX), 0x1bc1_8d91_0caf_e527);
    }

    #[test]
    fn seed_codecs_golden_values() {
        // The four folds `WorldCtor`/`WorldBaseCtor` perform on the seeds.
        assert_eq!(
            SEED_38.apply(0x627dce6e1d5603a4) ^ 0xd674a580a443b55e,
            0xa8b4_a54b_8140_1bd0
        );
        assert_eq!(
            (((SEED_B0.apply(0).wrapping_add(0xcf64407ada39d02e)) ^ 0xb2b338251ec2701)
                .rotate_left(0x17)),
            0x8242_eeb0_3b37_da9c
        );
        let u = ((SEED_A.apply(0) ^ 0x6e).wrapping_add(0x6c5df24a0815916e)) ^ 0x3c1986ede8c2b44a;
        let ror16 = u.rotate_right(0x10); // the constructor's `>> 0x10 | << 0x30`
        assert_eq!(
            (ror16 ^ 0xc2300451bcb9e837).wrapping_add(0x5c04429ca265e813),
            0x48ce_818a_ff58_32e7
        );
        assert_eq!(
            SEED_B.apply(0xf45f13eca6129815) ^ 0x689f22ff8960a2c7,
            0x9b3d_0f76_a220_e758
        );
    }

    #[test]
    fn operations_follow_the_instruction_semantics() {
        assert_eq!(Op::Ror(64).forward(1), 1, "ror masks the count to 6 bits");
        assert_eq!(Op::Shl(64).forward(1), 1);
        assert_eq!(Op::Add(u64::MAX).forward(1), 0);
        assert_eq!(Op::Neg.forward(1), u64::MAX);
    }
}
