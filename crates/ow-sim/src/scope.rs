//! The client's profiling scope stack, popped at `0x142cee300`.
//!
//! The container is a 0x28-byte header: `+0x08` and `+0x0c` are two running
//! totals, `+0x18` is the element array and `+0x20` the element count. An
//! element is 16 bytes: a `u32` at `+0x08` and a flag byte at `+0x0c`.
//!
//! The body computes the *last* element (`array + count*16`, then reads `-8`
//! and `-4`), and branches on the flag:
//!
//! ```text
//! flag == 0 : container[0x08] -= a ; container[0x0c] -= a ; count -= 1
//! flag != 0 : call 0x142cadf00(a's address?) ; container[0x0c] -= a ; count -= 1
//! ```
//!
//! The second path calls `0x142cadf00`, a guard-dispatch stub with **no
//! `.pdata` extent** that jumps through a global function pointer to
//! `0x141b328e0` - an import, and outside the gameplay scope. It is a named
//! open stage here (`None`), and the unit pins the flag to zero so the readable
//! path is what gets proved.

/// One pop of the scope stack.
///
/// `array` is the element array the container's `+0x18` points at; the caller
/// resolves that pointer, exactly as the probe resolves a guest address.
///
/// Returns `None` when the last element's flag is set: that path leaves through
/// the guard-dispatched thunk at `0x142cadf00`, which is not ported.
pub fn scope_pop(container: &mut [u8], array: &[u8]) -> Option<()> {
    if container.len() < 0x24 {
        return None;
    }
    let count = u32::from_le_bytes(container[0x20..0x24].try_into().unwrap()) as i64;
    // The client does `rdi = array + count*16`, then reads `[rdi-8]` and
    // `[rdi-4]`: the last element's `+0x08` and its flag at `+0x0c`.
    let at = count.checked_mul(16)?.checked_sub(8)?;
    if at < 0 {
        return None;
    }
    let at = at as usize;
    if at + 8 > array.len() {
        return None;
    }
    let a = u32::from_le_bytes(array[at..at + 4].try_into().unwrap());
    let flag = array[at + 4];
    let count = (count - 1) as u32;
    if flag != 0 {
        // Open: `call 0x142cadf00` (guard dispatch, no `.pdata` extent).
        return None;
    }
    let low = u32::from_le_bytes(container[0x08..0x0c].try_into().unwrap());
    let high = u32::from_le_bytes(container[0x0c..0x10].try_into().unwrap());
    container[0x08..0x0c].copy_from_slice(&low.wrapping_sub(a).to_le_bytes());
    container[0x0c..0x10].copy_from_slice(&high.wrapping_sub(a).to_le_bytes());
    container[0x20..0x24].copy_from_slice(&count.to_le_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::scope_pop;

    fn container(count: u32, low: u32, high: u32) -> Vec<u8> {
        let mut c = vec![0u8; 0x28];
        c[0x08..0x0c].copy_from_slice(&low.to_le_bytes());
        c[0x0c..0x10].copy_from_slice(&high.to_le_bytes());
        c[0x20..0x24].copy_from_slice(&count.to_le_bytes());
        c
    }

    #[test]
    fn a_clear_flag_subtracts_from_both_totals_and_decrements() {
        let mut c = container(2, 100, 200);
        let mut arr = vec![0u8; 32];
        arr[0x10 + 8..0x10 + 12].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(scope_pop(&mut c, &arr), Some(()));
        assert_eq!(u32::from_le_bytes(c[0x08..0x0c].try_into().unwrap()), 93);
        assert_eq!(u32::from_le_bytes(c[0x0c..0x10].try_into().unwrap()), 193);
        assert_eq!(u32::from_le_bytes(c[0x20..0x24].try_into().unwrap()), 1);
    }

    #[test]
    fn a_set_flag_is_the_open_thunk_path() {
        let mut c = container(1, 0, 0);
        let mut arr = vec![0u8; 16];
        arr[8 + 4] = 1;
        assert_eq!(scope_pop(&mut c, &arr), None);
    }

    #[test]
    fn the_totals_wrap_like_the_client() {
        let mut c = container(1, 1, 1);
        let mut arr = vec![0u8; 16];
        arr[8..12].copy_from_slice(&5u32.to_le_bytes());
        assert_eq!(scope_pop(&mut c, &arr), Some(()));
        assert_eq!(
            u32::from_le_bytes(c[0x08..0x0c].try_into().unwrap()),
            u32::MAX - 3
        );
    }
}
