//! The camera chain scale, read at `0x141ffe590`.
//!
//! A node carries a scale at `+0x270`, a link to its parent at `+0x298`, and
//! flags at `+0x230`. The body is four instructions of logic:
//!
//! ```text
//! float chain_scale(node):
//!     if node[0x298] == 0:                   return node[0x270]
//!     if node[0x230] & 0x400000000 != 0:     return node[0x270]
//!     return chain_scale(node[0x298]) * node[0x270]
//! ```
//!
//! Two early exits and a **recursive** multiply, in that order: the flag stops
//! the walk at that node rather than skipping it, so the product includes the
//! flagged node's own scale but none of its ancestors' - and the client tests
//! the link before the flag, so a node with no link never reads its flags.
//!
//! `base` is the guest base the probe resolves the snapshot against. The client
//! dereferences an absolute pointer; this subtracts the base instead, which is
//! the same arithmetic one translation step earlier. A link outside `block` is a
//! fixture error and panics rather than returning a made-up scale.

/// Byte offset of the flags word inside a node.
pub const FLAGS: usize = 0x230;
/// The bit that stops the walk.
pub const STOP: u64 = 0x4_0000_0000;
/// Byte offset of the scale inside a node.
pub const SCALE: usize = 0x270;
/// Byte offset of the link to the parent.
pub const LINK: usize = 0x298;

fn f32_at(block: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(block[at..at + 4].try_into().unwrap())
}

fn u64_at(block: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(block[at..at + 8].try_into().unwrap())
}

/// `0x141ffe590`: a node's scale times its ancestors', stopping at the first
/// node with no link or with [`STOP`] set.
pub fn chain_scale(block: &[u8], base: u64, node: u64) -> f32 {
    let at = (node - base) as usize;
    let scale = f32_at(block, at + SCALE);
    let link = u64_at(block, at + LINK);
    if link == 0 {
        return scale;
    }
    if u64_at(block, at + FLAGS) & STOP != 0 {
        return scale;
    }
    chain_scale(block, base, link) * scale
}

#[cfg(test)]
mod tests {
    use super::{chain_scale, FLAGS, LINK, SCALE, STOP};

    const BASE: u64 = 0x5000_0000_0000;
    const NODE: usize = 0x400;

    /// `scales` linked parent-ward; `stop_at` sets STOP on that index.
    fn chain(scales: &[f32], stop_at: Option<usize>) -> Vec<u8> {
        let mut block = vec![0u8; NODE * (scales.len() + 2)];
        for (i, scale) in scales.iter().enumerate() {
            let at = NODE * (i + 1);
            block[at + SCALE..at + SCALE + 4].copy_from_slice(&scale.to_le_bytes());
            let link = if i + 1 < scales.len() {
                BASE + (NODE * (i + 2)) as u64
            } else {
                0
            };
            block[at + LINK..at + LINK + 8].copy_from_slice(&link.to_le_bytes());
            if stop_at == Some(i) {
                block[at + FLAGS..at + FLAGS + 8].copy_from_slice(&STOP.to_le_bytes());
            }
        }
        block
    }

    fn root() -> u64 {
        BASE + NODE as u64
    }

    #[test]
    fn a_lone_node_returns_its_own_scale() {
        assert_eq!(chain_scale(&chain(&[2.5], None), BASE, root()), 2.5);
    }

    #[test]
    fn a_chain_multiplies_its_scales() {
        assert_eq!(
            chain_scale(&chain(&[2.0, 3.0, 0.5], None), BASE, root()),
            3.0
        );
    }

    #[test]
    fn the_stop_bit_ends_the_walk_but_keeps_its_own_scale() {
        // 2.0 * 3.0; the 10.0 ancestor is never reached
        assert_eq!(
            chain_scale(&chain(&[2.0, 3.0, 10.0], Some(1)), BASE, root()),
            6.0
        );
    }

    #[test]
    fn the_stop_bit_on_the_root_returns_the_root_scale() {
        assert_eq!(chain_scale(&chain(&[7.0, 3.0], Some(0)), BASE, root()), 7.0);
    }

    #[test]
    fn a_zero_scale_stays_zero() {
        assert_eq!(chain_scale(&chain(&[0.0, 5.0], None), BASE, root()), 0.0);
    }
}
