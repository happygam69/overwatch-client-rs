/* Shim so the Ghidra C decompiles compile unmodified.
 *
 * The decompiles in dev/decompiles/ are raw Ghidra output: they use Ghidra's
 * placeholder integer types and ABS, and they call sibling FUN_* functions by
 * their raw names. This header supplies the type aliases and nothing else -
 * no behaviour. Every function the decompile calls must already be declared
 * (usually by including the sibling decompile, or by a stub the harness
 * generates and reports; a stub is a stand-in and is labelled as such).
 *
 * Tier-1 verification compiles the decompile next to this header and checks
 * the result against the emulated real bytes. If the decompile is wrong the
 * candidate diverges and the harness says so - which is the point.
 */
#ifndef GHIDRA_SHIM_H
#define GHIDRA_SHIM_H

#include <stdint.h>

typedef uint8_t undefined1;
typedef uint16_t undefined2;
typedef uint32_t undefined4;
typedef uint64_t undefined8;
typedef int8_t byte;
typedef int32_t int_;
typedef int64_t longlong;
typedef uint64_t ulonglong;
typedef uint16_t ushort;
typedef uint32_t uint;

#ifndef ABS
#define ABS(x) ((x) < 0 ? -(x) : (x))
#endif

/* Ghidra prints raw address constants as DAT_*; the decompiles we verify
 * first are self-contained and do not reference them. If a future unit's
 * decompile does, the unit loader must resolve the address against the
 * image and fail loudly rather than let this become a silent zero. */
#define GHIDRA_SHIM_UNRESOLVED_DAT 0

#endif /* GHIDRA_SHIM_H */
