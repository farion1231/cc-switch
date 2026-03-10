/* Copyright (c) 2023, Google Inc.
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
 * OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
 * CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE. */

#ifndef OPENSSL_HEADER_ASM_BASE_H
#define OPENSSL_HEADER_ASM_BASE_H

#include <openssl/target.h>

// This header contains symbols and common sections used by assembly files. It
// is included as a public header to simplify the build, but is not intended for
// external use.
//
// Every assembly file must include this header. Some linker features require
// all object files to be tagged with some section metadata. This header file,
// when included in assembly, adds that metadata. It also makes defines like
// |OPENSSL_X86_64| available and includes the prefixing macros.
//
// Including this header in an assembly file imples:
//
// - The file does not require an executable stack.
//
// - The file, on aarch64, uses the macros defined below to be compatible with
//   BTI and PAC.
//
// - The file, on x86_64, requires the program to be compatible with Intel IBT
//   and SHSTK

#if defined(__ASSEMBLER__)

#include <openssl/boringssl_prefix_symbols_asm.h>

#if defined(__ELF__)
// Every ELF object file, even empty ones, should disable executable stacks. See
// https://www.airs.com/blog/archives/518.
.pushsection .note.GNU-stack, "", %progbits
.popsection
#endif

#if defined(__CET__) && defined(OPENSSL_X86_64)
// Clang and GCC define __CET__ and provide <cet.h> when they support Intel's
// Indirect Branch Tracking.
// https://lpc.events/event/7/contributions/729/attachments/496/903/CET-LPC-2020.pdf
//
// cet.h defines _CET_ENDBR which is used to mark function entry points for IBT.
// and adds the assembly marker. The value of _CET_ENDBR is made dependant on if
// '-fcf-protection' is passed to the compiler. _CET_ENDBR is only required when
// the function is the target of an indirect jump, but BoringSSL chooses to mark
// all assembly entry points because it is easier, and allows BoringSSL's ABI
// tester to call the assembly entry points via an indirect jump.
#include <cet.h>
#elif !defined(_CET_ENDBR)
// If cet.h does not exist, manually define _CET_ENDBR to be the ENDBR64
// instruction, with an explicit byte sequence for compilers/assemblers that
// don't know about it. Note that it is safe to use ENDBR64 on all platforms,
// since the encoding is by design interpreted as a NOP on all pre-CET x86_64
// processors.
#define _CET_ENDBR .byte 0xf3,0x0f,0x1e,0xfa
#endif

#if defined(OPENSSL_ARM) || defined(OPENSSL_AARCH64)

// We require the ARM assembler provide |__ARM_ARCH| from Arm C Language
// Extensions (ACLE). This is supported in GCC 4.8+ and Clang 3.2+. MSVC does
// not implement ACLE, but we require Clang's assembler on Windows.
#if !defined(__ARM_ARCH)
#error "ARM assembler must define __ARM_ARCH"
#endif

// Even when building for 32-bit ARM, support for aarch64 crypto instructions
// will be included.
//
// TODO(davidben): Remove this and the corresponding ifdefs? This is only
// defined because some OpenSSL assembly files would allow disabling the NEON
// code entirely. I think we'd prefer to do that by lifting the dispatch to C
// anyway.
#define __ARM_MAX_ARCH__ 8

// Support macros for
//   - Armv8.3-A Pointer Authentication and
//   - Armv8.5-A Branch Target Identification
// features which require emitting a .note.gnu.property section with the
// appropriate architecture-dependent feature bits set.
//
// |AARCH64_SIGN_LINK_REGISTER| and |AARCH64_VALIDATE_LINK_REGISTER| expand to
// PACIxSP and AUTIxSP, respectively. |AARCH64_SIGN_LINK_REGISTER| should be
// used immediately before saving the LR register (x30) to the stack.
// |AARCH64_VALIDATE_LINK_REGISTER| should be used immediately after restoring
// it. Note |AARCH64_SIGN_LINK_REGISTER|'s modifications to LR must be undone
// with |AARCH64_VALIDATE_LINK_REGISTER| before RET. The SP register must also
// have the same value at the two points. For example:
//
//   .global f
//   f:
//     AARCH64_SIGN_LINK_REGISTER
//     stp x29, x30, [sp, #-96]!
//     mov x29, sp
//     ...
//     ldp x29, x30, [sp], #96
//     AARCH64_VALIDATE_LINK_REGISTER
//     ret
//
// |AARCH64_VALID_CALL_TARGET| expands to BTI 'c'. Either it, or
// |AARCH64_SIGN_LINK_REGISTER|, must be used at every point that may be an
// indirect call target. In particular, all symbols exported from a file must
// begin with one of these macros. For example, a leaf function that does not
// save LR can instead use |AARCH64_VALID_CALL_TARGET|:
//
//   .globl return_zero
//   return_zero:
//     AARCH64_VALID_CALL_TARGET
//     mov x0, #0
//     ret
//
// A non-leaf function which does not immediately save LR may need both macros
// because |AARCH64_SIGN_LINK_REGISTER| appears late. For example, the function
// may jump to an alternate implementation before setting up the stack:
//
//   .globl with_early_jump
//   with_early_jump:
//     AARCH64_VALID_CALL_TARGET
//     cmp x0, #128
//     b.lt .Lwith_early_jump_128
//     AARCH64_SIGN_LINK_REGISTER
//     stp x29, x30, [sp, #-96]!
//     mov x29, sp
//     ...
//     ldp x29, x30, [sp], #96
//     AARCH64_VALIDATE_LINK_REGISTER
//     ret
//
//  .Lwith_early_jump_128:
//     ...
//     ret
//
// These annotations are only required with indirect calls. Private symbols that
// are only the target of direct calls do not require annotations. Also note
// that |AARCH64_VALID_CALL_TARGET| is only valid for indirect calls (BLR), not
// indirect jumps (BR). Indirect jumps in assembly are currently not supported
// and would require a macro for BTI 'j'.
//
// Although not necessary, it is safe to use these macros in 32-bit ARM
// assembly. This may be used to simplify dual 32-bit and 64-bit files.
//
// References:
// - "ELF for the Arm® 64-bit Architecture"
//   https://github.com/ARM-software/abi-aa/blob/master/aaelf64/aaelf64.rst
// - "Providing protection for complex software"
//   https://developer.arm.com/architectures/learn-the-architecture/providing-protection-for-complex-software

#if defined(__ARM_FEATURE_BTI_DEFAULT) && __ARM_FEATURE_BTI_DEFAULT == 1
#define GNU_PROPERTY_AARCH64_BTI (1 << 0)   // Has Branch Target Identification
#define AARCH64_VALID_CALL_TARGET hint #34  // BTI 'c'
#else
#define GNU_PROPERTY_AARCH64_BTI 0  // No Branch Target Identification
#define AARCH64_VALID_CALL_TARGET
#endif

#if defined(__ARM_FEATURE_PAC_DEFAULT) && \
    (__ARM_FEATURE_PAC_DEFAULT & 1) == 1  // Signed with A-key
#define GNU_PROPERTY_AARCH64_POINTER_AUTH \
  (1 << 1)                                       // Has Pointer Authentication
#define AARCH64_SIGN_LINK_REGISTER hint #25      // PACIASP
#define AARCH64_VALIDATE_LINK_REGISTER hint #29  // AUTIASP
#elif defined(__ARM_FEATURE_PAC_DEFAULT) && \
    (__ARM_FEATURE_PAC_DEFAULT & 2) == 2  // Signed with B-key
#define GNU_PROPERTY_AARCH64_POINTER_AUTH \
  (1 << 1)                                       // Has Pointer Authentication
#define AARCH64_SIGN_LINK_REGISTER hint #27      // PACIBSP
#define AARCH64_VALIDATE_LINK_REGISTER hint #31  // AUTIBSP
#else
#define GNU_PROPERTY_AARCH64_POINTER_AUTH 0  // No Pointer Authentication
#if GNU_PROPERTY_AARCH64_BTI != 0
#define AARCH64_SIGN_LINK_REGISTER AARCH64_VALID_CALL_TARGET
#else
#define AARCH64_SIGN_LINK_REGISTER
#endif
#define AARCH64_VALIDATE_LINK_REGISTER
#endif

#if GNU_PROPERTY_AARCH64_POINTER_AUTH != 0 || GNU_PROPERTY_AARCH64_BTI != 0
.pushsection .note.gnu.property, "a";
.balign 8;
.long 4;
.long 0x10;
.long 0x5;
.asciz "GNU";
.long 0xc0000000; /* GNU_PROPERTY_AARCH64_FEATURE_1_AND */
.long 4;
.long (GNU_PROPERTY_AARCH64_POINTER_AUTH | GNU_PROPERTY_AARCH64_BTI);
.long 0;
.popsection;
#endif
#endif  // ARM || AARCH64

#endif  // __ASSEMBLER__

#endif  // OPENSSL_HEADER_ASM_BASE_H
