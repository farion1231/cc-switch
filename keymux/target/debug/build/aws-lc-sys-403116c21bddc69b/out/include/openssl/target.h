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

#ifndef OPENSSL_HEADER_TARGET_H
#define OPENSSL_HEADER_TARGET_H

// Preprocessor symbols that define the target platform.
//
// This file may be included in C, C++, and assembler and must be compatible
// with each environment. It is separated out only to share code between
// <openssl/base.h> and <openssl/asm_base.h>. Prefer to include those headers
// instead.

#if defined(__BYTE_ORDER__) && defined(__ORDER_BIG_ENDIAN__)
#  if __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
#    define OPENSSL_BIG_ENDIAN
#  endif
#elif !defined(_MSC_VER) && defined(__has_include)
#  if __has_include(<endian.h>)
#    include <endian.h>
#  elif __has_include(<sys/param.h>)
#    include <sys/param.h>
#  endif
#  if (defined(__BYTE_ORDER) && defined(__BIG_ENDIAN) && __BYTE_ORDER == __BIG_ENDIAN) || \
      (defined(_BYTE_ORDER) && defined(_BIG_ENDIAN) && _BYTE_ORDER == _BIG_ENDIAN) || \
      (defined(BYTE_ORDER)  && defined(BIG_ENDIAN)  && BYTE_ORDER == BIG_ENDIAN)
#    define OPENSSL_BIG_ENDIAN
#  endif
#elif defined(_M_PPC)
#  define OPENSSL_BIG_ENDIAN
#endif

#if (defined(__SIZEOF_POINTER__) && __SIZEOF_POINTER__ == 8) || defined(__LP64__) || defined(_WIN64)
#define OPENSSL_64_BIT
#else
#define OPENSSL_32_BIT
#endif

#if defined(__x86_64) || defined(_M_AMD64) || defined(_M_X64)
#define OPENSSL_X86_64
#elif defined(__x86) || defined(__i386) || defined(__i386__) || defined(_M_IX86)
#define OPENSSL_X86
#elif defined(__AARCH64EL__) || defined(_M_ARM64)
#define OPENSSL_AARCH64
#elif defined(__ARMEL__) || defined(_M_ARM)
#define OPENSSL_ARM
#elif defined(OPENSSL_64_BIT) && (defined(__powerpc__) || defined(__ppc__) || defined(_ARCH_PPC) || \
    defined(__PPC__) || defined(__ppc64__) || defined(__PPC64__) || defined(_ARCH_PPC64)  || \
    defined(__ppc64le__) || defined(__powerpc64__) || defined(_M_PPC))
#  if defined(OPENSSL_BIG_ENDIAN)
#    define OPENSSL_PPC64BE
#  else
#    define OPENSSL_PPC64LE
#  endif
#elif defined(__powerpc__) || defined(__ppc__) || defined(_ARCH_PPC) || defined(__PPC__) || defined(_M_PPC)
#  if defined(OPENSSL_BIG_ENDIAN)
#    define OPENSSL_PPC32BE
#  else
#   define OPENSSL_PPC32LE
#  endif
#elif defined(__s390x__)
#define OPENSSL_S390X
#elif defined(__sparc__)
#define OPENSSL_SPARCV9
#elif defined(__MIPSEL__) || defined(__MIPSEB__)
#  if defined(OPENSSL_64_BIT)
#    define OPENSSL_MIPS64
#  else
#    define OPENSSL_MIPS
#  endif
#elif defined(__riscv)
#  if defined(OPENSSL_64_BIT)
#    define OPENSSL_RISCV64
#  endif
#elif defined(__loongarch_lp64)
#define OPENSSL_LOONGARCH64
#elif defined(__pnacl__)
#define OPENSSL_PNACL
#elif defined(__wasm__) // Allowed but no macro defined
#elif defined(__asmjs__) // Allowed but no macro defined
#elif defined(__myriad2__) // Allowed but no macro defined
#else
// Run the crypto_test binary, notably crypto/compiler_test.cc, before adding
// a new architecture.
#error "Unknown target CPU"
#endif

#if defined(__APPLE__)
#define OPENSSL_APPLE
// Note |TARGET_OS_MAC| is set for all Apple OS variants. |TARGET_OS_OSX|
// targets macOS specifically.
#if defined(TARGET_OS_OSX) && TARGET_OS_OSX
#define OPENSSL_MACOS
#endif
#if defined(TARGET_OS_IPHONE) && TARGET_OS_IPHONE
#define OPENSSL_IOS
#endif
#endif

#if defined(_WIN32)
#define OPENSSL_WINDOWS
#endif

// Android baremetal aren't Linux but currently define __linux__.
// As a workaround, we exclude them here.
// We also exclude nanolibc/CrOS EC/Zephyr. nanolibc/CrOS EC/Zephyr
// sometimes build for a non-Linux target (which should not define __linux__),
// but also sometimes build for Linux. Although technically running in Linux
// userspace, this lacks all the libc APIs we'd normally expect on Linux, so we
// treat it as a non-Linux target.
//
// TODO(b/291101350): Remove this workaround once Android baremetal no longer
// defines it.
#if defined(__linux__) && \
    !defined(ANDROID_BAREMETAL) && !defined(OPENSSL_NANOLIBC) && \
    !defined(CROS_EC) && !defined(CROS_ZEPHYR)
#define OPENSSL_LINUX
#endif

// nanolibc is a particular minimal libc implementation. Defining this on any
// other platform is not supported. Other embedded platforms must introduce
// their own defines.
#if defined(OPENSSL_NANOLIBC)
#define OPENSSL_NO_FILESYSTEM
#define OPENSSL_NO_POSIX_IO
#define OPENSSL_NO_SOCK
#define OPENSSL_NO_THREADS_CORRUPT_MEMORY_AND_LEAK_SECRETS_IF_THREADED
#endif

// Android baremetal is an embedded target that uses a subset of bionic.
// Defining this on any other platform is not supported. Other embedded
// platforms must introduce their own defines.
#if defined(ANDROID_BAREMETAL)
#define OPENSSL_NO_FILESYSTEM
#define OPENSSL_NO_POSIX_IO
#define OPENSSL_NO_SOCK
#define OPENSSL_NO_THREADS_CORRUPT_MEMORY_AND_LEAK_SECRETS_IF_THREADED
#endif

// CROS_EC is an embedded target for ChromeOS Embedded Controller. Defining
// this on any other platform is not supported. Other embedded platforms must
// introduce their own defines.
//
// https://chromium.googlesource.com/chromiumos/platform/ec/+/HEAD/README.md
#if defined(CROS_EC)
#define OPENSSL_NO_FILESYSTEM
#define OPENSSL_NO_POSIX_IO
#define OPENSSL_NO_SOCK
#define OPENSSL_NO_THREADS_CORRUPT_MEMORY_AND_LEAK_SECRETS_IF_THREADED
#endif

// CROS_ZEPHYR is an embedded target for ChromeOS Zephyr Embedded Controller.
// Defining this on any other platform is not supported. Other embedded
// platforms must introduce their own defines.
//
// https://chromium.googlesource.com/chromiumos/platform/ec/+/HEAD/docs/zephyr/README.md
#if defined(CROS_ZEPHYR)
#define OPENSSL_NO_FILESYSTEM
#define OPENSSL_NO_POSIX_IO
#define OPENSSL_NO_SOCK
#define OPENSSL_NO_THREADS_CORRUPT_MEMORY_AND_LEAK_SECRETS_IF_THREADED
#endif

#if defined(__ANDROID_API__)
#define OPENSSL_ANDROID
#endif

#if defined(__FreeBSD__)
#define OPENSSL_FREEBSD
#endif

#if defined(__OpenBSD__)
#define OPENSSL_OPENBSD
#endif

#if defined(__NetBSD__)
#define OPENSSL_NETBSD
#endif

#if defined(__illumos__) || (defined(__sun) && defined(__SVR4))
#define OPENSSL_SOLARIS
#endif

// BoringSSL requires platform's locking APIs to make internal global state
// thread-safe, including the PRNG. On some single-threaded embedded platforms,
// locking APIs may not exist, so this dependency may be disabled with the
// following build flag.
//
// IMPORTANT: Doing so means the consumer promises the library will never be
// used in any multi-threaded context. It causes BoringSSL to be globally
// thread-unsafe. Setting it inappropriately will subtly and unpredictably
// corrupt memory and leak secret keys.
//
// Do not set this flag on any platform where threads are possible. BoringSSL
// maintainers will not provide support for any consumers that do so. Changes
// which break such unsupported configurations will not be reverted.
#if !defined(OPENSSL_NO_THREADS_CORRUPT_MEMORY_AND_LEAK_SECRETS_IF_THREADED)
#define OPENSSL_THREADS
#endif

#if defined(BORINGSSL_UNSAFE_FUZZER_MODE) && \
    !defined(BORINGSSL_UNSAFE_DETERMINISTIC_MODE)
#define BORINGSSL_UNSAFE_DETERMINISTIC_MODE
#endif

#if defined(__has_feature)
#if __has_feature(address_sanitizer)
#define OPENSSL_ASAN
#endif
#if __has_feature(thread_sanitizer)
#define OPENSSL_TSAN
#endif
#if __has_feature(memory_sanitizer)
#define OPENSSL_MSAN
#define OPENSSL_ASM_INCOMPATIBLE
#endif
#if __has_feature(hwaddress_sanitizer)
#define OPENSSL_HWASAN
#endif
#endif

// Disable 32-bit Arm assembly on Apple platforms. The last iOS version that
// supported 32-bit Arm was iOS 10.
#if defined(OPENSSL_APPLE) && defined(OPENSSL_ARM)
#define OPENSSL_ASM_INCOMPATIBLE
#endif

#if defined(OPENSSL_ASM_INCOMPATIBLE)
#undef OPENSSL_ASM_INCOMPATIBLE
#if !defined(OPENSSL_NO_ASM)
#define OPENSSL_NO_ASM
#endif
#endif  // OPENSSL_ASM_INCOMPATIBLE

#endif  // OPENSSL_HEADER_TARGET_H
