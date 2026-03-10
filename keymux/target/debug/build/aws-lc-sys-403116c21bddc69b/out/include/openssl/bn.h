/* Copyright (C) 1995-1997 Eric Young (eay@cryptsoft.com)
 * All rights reserved.
 *
 * This package is an SSL implementation written
 * by Eric Young (eay@cryptsoft.com).
 * The implementation was written so as to conform with Netscapes SSL.
 *
 * This library is free for commercial and non-commercial use as long as
 * the following conditions are aheared to.  The following conditions
 * apply to all code found in this distribution, be it the RC4, RSA,
 * lhash, DES, etc., code; not just the SSL code.  The SSL documentation
 * included with this distribution is covered by the same copyright terms
 * except that the holder is Tim Hudson (tjh@cryptsoft.com).
 *
 * Copyright remains Eric Young's, and as such any Copyright notices in
 * the code are not to be removed.
 * If this package is used in a product, Eric Young should be given attribution
 * as the author of the parts of the library used.
 * This can be in the form of a textual message at program startup or
 * in documentation (online or textual) provided with the package.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *    "This product includes cryptographic software written by
 *     Eric Young (eay@cryptsoft.com)"
 *    The word 'cryptographic' can be left out if the rouines from the library
 *    being used are not cryptographic related :-).
 * 4. If you include any Windows specific code (or a derivative thereof) from
 *    the apps directory (application code) you must include an acknowledgement:
 *    "This product includes software written by Tim Hudson (tjh@cryptsoft.com)"
 *
 * THIS SOFTWARE IS PROVIDED BY ERIC YOUNG ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * The licence and distribution terms for any publically available version or
 * derivative of this code cannot be changed.  i.e. this code cannot simply be
 * copied and put under another distribution licence
 * [including the GNU Public Licence.]
 */
/* ====================================================================
 * Copyright (c) 1998-2006 The OpenSSL Project.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 *
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in
 *    the documentation and/or other materials provided with the
 *    distribution.
 *
 * 3. All advertising materials mentioning features or use of this
 *    software must display the following acknowledgment:
 *    "This product includes software developed by the OpenSSL Project
 *    for use in the OpenSSL Toolkit. (http://www.openssl.org/)"
 *
 * 4. The names "OpenSSL Toolkit" and "OpenSSL Project" must not be used to
 *    endorse or promote products derived from this software without
 *    prior written permission. For written permission, please contact
 *    openssl-core@openssl.org.
 *
 * 5. Products derived from this software may not be called "OpenSSL"
 *    nor may "OpenSSL" appear in their names without prior written
 *    permission of the OpenSSL Project.
 *
 * 6. Redistributions of any form whatsoever must retain the following
 *    acknowledgment:
 *    "This product includes software developed by the OpenSSL Project
 *    for use in the OpenSSL Toolkit (http://www.openssl.org/)"
 *
 * THIS SOFTWARE IS PROVIDED BY THE OpenSSL PROJECT ``AS IS'' AND ANY
 * EXPRESSED OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE OpenSSL PROJECT OR
 * ITS CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
 * OF THE POSSIBILITY OF SUCH DAMAGE.
 * ====================================================================
 *
 * This product includes cryptographic software written by Eric Young
 * (eay@cryptsoft.com).  This product includes software written by Tim
 * Hudson (tjh@cryptsoft.com).
 *
 */
/* ====================================================================
 * Copyright 2002 Sun Microsystems, Inc. ALL RIGHTS RESERVED.
 *
 * Portions of the attached software ("Contribution") are developed by
 * SUN MICROSYSTEMS, INC., and are contributed to the OpenSSL project.
 *
 * The Contribution is licensed pursuant to the Eric Young open source
 * license provided above.
 *
 * The binary polynomial arithmetic software is originally written by
 * Sheueling Chang Shantz and Douglas Stebila of Sun Microsystems
 * Laboratories. */

#ifndef OPENSSL_HEADER_BN_H
#define OPENSSL_HEADER_BN_H

#include <openssl/base.h>
#include <openssl/thread.h>

#ifndef __STDC_FORMAT_MACROS
#define __STDC_FORMAT_MACROS
#endif
#include <inttypes.h>  // for PRIu64 and friends

#include <stdio.h>  // for FILE*

#if defined(__cplusplus)
extern "C" {
#endif


// BN provides support for working with arbitrary sized integers. For example,
// although the largest integer supported by the compiler might be 64 bits, BN
// will allow you to work with much larger numbers.
//
// This library is developed for use inside BoringSSL, and uses implementation
// strategies that may not be ideal for other applications. Non-cryptographic
// uses should use a more general-purpose integer library, especially if
// performance-sensitive.
//
// Many functions in BN scale quadratically or higher in the bit length of their
// input. Callers at this layer are assumed to have capped input sizes within
// their performance tolerances.


// BN_ULONG is the native word size when working with big integers.
//
// Note: on some platforms, inttypes.h does not define print format macros in
// C++ unless |__STDC_FORMAT_MACROS| defined. This is due to text in C99 which
// was never adopted in any C++ standard and explicitly overruled in C++11. As
// this is a public header, bn.h does not define |__STDC_FORMAT_MACROS| itself.
// Projects which use |BN_*_FMT*| with outdated C headers may need to define it
// externally.
#if defined(OPENSSL_64_BIT)
typedef uint64_t BN_ULONG;
#define BN_BITS2 64
#define BN_DEC_FMT1 "%" PRIu64
#define BN_HEX_FMT1 "%" PRIx64
#define BN_HEX_FMT2 "%016" PRIx64
#elif defined(OPENSSL_32_BIT)
typedef uint32_t BN_ULONG;
#define BN_BITS2 32
#define BN_DEC_FMT1 "%" PRIu32
#define BN_HEX_FMT1 "%" PRIx32
#define BN_HEX_FMT2 "%08" PRIx32
#else
#error "Must define either OPENSSL_32_BIT or OPENSSL_64_BIT"
#endif


// Allocation and freeing.

// BN_new creates a new, allocated BIGNUM and initialises it.
OPENSSL_EXPORT BIGNUM *BN_new(void);

// BN_init initialises a stack allocated |BIGNUM|.
OPENSSL_EXPORT void BN_init(BIGNUM *bn);

// BN_free frees the data referenced by |bn| and, if |bn| was originally
// allocated on the heap, frees |bn| also.
OPENSSL_EXPORT void BN_free(BIGNUM *bn);

// BN_clear_free erases and frees the data referenced by |bn| and, if |bn| was
// originally allocated on the heap, frees |bn| also.
OPENSSL_EXPORT void BN_clear_free(BIGNUM *bn);

// BN_dup allocates a new BIGNUM and sets it equal to |src|. It returns the
// allocated BIGNUM on success or NULL otherwise.
OPENSSL_EXPORT BIGNUM *BN_dup(const BIGNUM *src);

// BN_copy sets |dest| equal to |src| and returns |dest| or NULL on allocation
// failure.
OPENSSL_EXPORT BIGNUM *BN_copy(BIGNUM *dest, const BIGNUM *src);

// BN_clear sets |bn| to zero and erases the old data.
OPENSSL_EXPORT void BN_clear(BIGNUM *bn);

// BN_value_one returns a static BIGNUM with value 1.
OPENSSL_EXPORT const BIGNUM *BN_value_one(void);


// Basic functions.

// BN_num_bits returns the minimum number of bits needed to represent the
// absolute value of |bn|.
OPENSSL_EXPORT unsigned BN_num_bits(const BIGNUM *bn);

// BN_num_bytes returns the minimum number of bytes needed to represent the
// absolute value of |bn|.
//
// While |size_t| is the preferred type for byte counts, callers can assume that
// |BIGNUM|s are bounded such that this value, and its corresponding bit count,
// will always fit in |int|.
OPENSSL_EXPORT unsigned BN_num_bytes(const BIGNUM *bn);

// BN_get_minimal_width returns the minimal number of words needed to represent
// |bn|. This function can leak the size of the value encoded in |BN|.
OPENSSL_EXPORT int BN_get_minimal_width(const BIGNUM *bn);

// BN_zero sets |bn| to zero.
OPENSSL_EXPORT void BN_zero(BIGNUM *bn);

// BN_one sets |bn| to one. It returns one on success or zero on allocation
// failure.
OPENSSL_EXPORT int BN_one(BIGNUM *bn);

// BN_set_word sets |bn| to |value|. It returns one on success or zero on
// allocation failure.
OPENSSL_EXPORT int BN_set_word(BIGNUM *bn, BN_ULONG value);

// BN_set_u64 sets |bn| to |value|. It returns one on success or zero on
// allocation failure.
OPENSSL_EXPORT int BN_set_u64(BIGNUM *bn, uint64_t value);

// BN_set_negative sets the sign of |bn|.
OPENSSL_EXPORT void BN_set_negative(BIGNUM *bn, int sign);

// BN_is_negative returns one if |bn| is negative and zero otherwise.
OPENSSL_EXPORT int BN_is_negative(const BIGNUM *bn);


// Conversion functions.

// BN_bin2bn sets |*ret| to the value of |len| bytes from |in|, interpreted as
// a big-endian number, and returns |ret|. If |ret| is NULL then a fresh
// |BIGNUM| is allocated and returned. It returns NULL on allocation
// failure.
OPENSSL_EXPORT BIGNUM *BN_bin2bn(const uint8_t *in, size_t len, BIGNUM *ret);

// BN_bn2bin serialises the absolute value of |in| to |out| as a big-endian
// integer, which must have |BN_num_bytes| of space available. It returns the
// number of bytes written. Note this function leaks the magnitude of |in|. If
// |in| is secret, use |BN_bn2bin_padded| instead.
OPENSSL_EXPORT size_t BN_bn2bin(const BIGNUM *in, uint8_t *out);

// BN_le2bn sets |*ret| to the value of |len| bytes from |in|, interpreted as
// a little-endian number, and returns |ret|. If |ret| is NULL then a fresh
// |BIGNUM| is allocated and returned. It returns NULL on allocation
// failure.
OPENSSL_EXPORT BIGNUM *BN_le2bn(const uint8_t *in, size_t len, BIGNUM *ret);

// BN_bn2le_padded serialises the absolute value of |in| to |out| as a
// little-endian integer, which must have |len| of space available, padding
// out the remainder of out with zeros. If |len| is smaller than |BN_num_bytes|,
// the function fails and returns 0. Otherwise, it returns 1.
OPENSSL_EXPORT int BN_bn2le_padded(uint8_t *out, size_t len, const BIGNUM *in);

// BN_bn2bin_padded serialises the absolute value of |in| to |out| as a
// big-endian integer. The integer is padded with leading zeros up to size
// |len|. If |len| is smaller than |BN_num_bytes|, the function fails and
// returns 0. Otherwise, it returns 1.
OPENSSL_EXPORT int BN_bn2bin_padded(uint8_t *out, size_t len, const BIGNUM *in);

// BN_bn2cbb_padded behaves like |BN_bn2bin_padded| but writes to a |CBB|.
OPENSSL_EXPORT int BN_bn2cbb_padded(CBB *out, size_t len, const BIGNUM *in);

// BN_bn2hex returns an allocated string that contains a NUL-terminated, hex
// representation of |bn|. If |bn| is negative, the first char in the resulting
// string will be '-'. Returns NULL on allocation failure.
OPENSSL_EXPORT char *BN_bn2hex(const BIGNUM *bn);

// BN_hex2bn parses the leading hex number from |in|, which may be proceeded by
// a '-' to indicate a negative number and may contain trailing, non-hex data.
// If |outp| is not NULL, it constructs a BIGNUM equal to the hex number and
// stores it in |*outp|. If |*outp| is NULL then it allocates a new BIGNUM and
// updates |*outp|. It returns the number of bytes of |in| processed or zero on
// error.
OPENSSL_EXPORT int BN_hex2bn(BIGNUM **outp, const char *in);

// BN_bn2dec returns an allocated string that contains a NUL-terminated,
// decimal representation of |bn|. If |bn| is negative, the first char in the
// resulting string will be '-'. Returns NULL on allocation failure.
//
// Converting an arbitrarily large integer to decimal is quadratic in the bit
// length of |a|. This function assumes the caller has capped the input within
// performance tolerances.
OPENSSL_EXPORT char *BN_bn2dec(const BIGNUM *a);

// BN_dec2bn parses the leading decimal number from |in|, which may be
// proceeded by a '-' to indicate a negative number and may contain trailing,
// non-decimal data. If |outp| is not NULL, it constructs a BIGNUM equal to the
// decimal number and stores it in |*outp|. If |*outp| is NULL then it
// allocates a new BIGNUM and updates |*outp|. It returns the number of bytes
// of |in| processed or zero on error.
//
// Converting an arbitrarily large integer to decimal is quadratic in the bit
// length of |a|. This function assumes the caller has capped the input within
// performance tolerances.
OPENSSL_EXPORT int BN_dec2bn(BIGNUM **outp, const char *in);

// BN_asc2bn acts like |BN_dec2bn| or |BN_hex2bn| depending on whether |in|
// begins with "0X" or "0x" (indicating hex) or not (indicating decimal). A
// leading '-' is still permitted and comes before the optional 0X/0x. It
// returns one on success or zero on error.
OPENSSL_EXPORT int BN_asc2bn(BIGNUM **outp, const char *in);

// BN_print writes a hex encoding of |a| to |bio|. It returns one on success
// and zero on error.
OPENSSL_EXPORT int BN_print(BIO *bio, const BIGNUM *a);

// BN_print_fp acts like |BIO_print|, but wraps |fp| in a |BIO| first.
OPENSSL_EXPORT int BN_print_fp(FILE *fp, const BIGNUM *a);

// BN_get_word returns the absolute value of |bn| as a single word. If |bn| is
// too large to be represented as a single word, the maximum possible value
// will be returned.
OPENSSL_EXPORT BN_ULONG BN_get_word(const BIGNUM *bn);

// BN_get_u64 sets |*out| to the absolute value of |bn| as a |uint64_t| and
// returns one. If |bn| is too large to be represented as a |uint64_t|, it
// returns zero.
OPENSSL_EXPORT int BN_get_u64(const BIGNUM *bn, uint64_t *out);

// BN_get_flags interprets |flags| as a bitmask and returns the flags for |bn|.
// The returned value is a set of bitmask of |BN_FLG_*| values, ORed together,
// or 0 if none of the given flags are set.
OPENSSL_EXPORT int BN_get_flags(const BIGNUM *bn, int flags);


// ASN.1 functions.

// BN_parse_asn1_unsigned parses a non-negative DER INTEGER from |cbs| writes
// the result to |ret|. It returns one on success and zero on failure.
OPENSSL_EXPORT int BN_parse_asn1_unsigned(CBS *cbs, BIGNUM *ret);

// BN_marshal_asn1 marshals |bn| as a non-negative DER INTEGER and appends the
// result to |cbb|. It returns one on success and zero on failure.
OPENSSL_EXPORT int BN_marshal_asn1(CBB *cbb, const BIGNUM *bn);


// BIGNUM pools.
//
// Certain BIGNUM operations need to use many temporary variables and
// allocating and freeing them can be quite slow. Thus such operations typically
// take a |BN_CTX| parameter, which contains a pool of |BIGNUMs|. The |ctx|
// argument to a public function may be NULL, in which case a local |BN_CTX|
// will be created just for the lifetime of that call.
//
// A function must call |BN_CTX_start| first. Then, |BN_CTX_get| may be called
// repeatedly to obtain temporary |BIGNUM|s. All |BN_CTX_get| calls must be made
// before calling any other functions that use the |ctx| as an argument.
//
// Finally, |BN_CTX_end| must be called before returning from the function.
// When |BN_CTX_end| is called, the |BIGNUM| pointers obtained from
// |BN_CTX_get| become invalid.

// BN_CTX_new returns a new, empty BN_CTX or NULL on allocation failure.
OPENSSL_EXPORT BN_CTX *BN_CTX_new(void);

// BN_CTX_free frees all BIGNUMs contained in |ctx| and then frees |ctx|
// itself.
OPENSSL_EXPORT void BN_CTX_free(BN_CTX *ctx);

// BN_CTX_start "pushes" a new entry onto the |ctx| stack and allows future
// calls to |BN_CTX_get|.
OPENSSL_EXPORT void BN_CTX_start(BN_CTX *ctx);

// BN_CTX_get returns a new |BIGNUM|, or NULL on allocation failure. Once
// |BN_CTX_get| has returned NULL, all future calls will also return NULL until
// |BN_CTX_end| is called.
OPENSSL_EXPORT BIGNUM *BN_CTX_get(BN_CTX *ctx);

// BN_CTX_end invalidates all |BIGNUM|s returned from |BN_CTX_get| since the
// matching |BN_CTX_start| call.
OPENSSL_EXPORT void BN_CTX_end(BN_CTX *ctx);


// Simple arithmetic

// BN_add sets |r| = |a| + |b|, where |r| may be the same pointer as either |a|
// or |b|. It returns one on success and zero on allocation failure.
// The size of |a| and |b| are assumed to be public.
OPENSSL_EXPORT int BN_add(BIGNUM *r, const BIGNUM *a, const BIGNUM *b);

// BN_uadd sets |r| = |a| + |b|, where |a| and |b| are non-negative and |r| may
// be the same pointer as either |a| or |b|. It returns one on success and zero
// on allocation failure.
// The size of |a| and |b| are assumed to be public.
OPENSSL_EXPORT int BN_uadd(BIGNUM *r, const BIGNUM *a, const BIGNUM *b);

// BN_add_word adds |w| to |a|. It returns one on success and zero otherwise.
// The size of |a| and |w| are assumed to be public.
OPENSSL_EXPORT int BN_add_word(BIGNUM *a, BN_ULONG w);

// BN_sub sets |r| = |a| - |b|, where |r| may be the same pointer as either |a|
// or |b|. It returns one on success and zero on allocation failure.
// The size of |a| and |b| are assumed to be public.
OPENSSL_EXPORT int BN_sub(BIGNUM *r, const BIGNUM *a, const BIGNUM *b);

// BN_usub sets |r| = |a| - |b|, where |a| and |b| are non-negative integers,
// |b| < |a| and |r| may be the same pointer as either |a| or |b|. It returns
// one on success and zero on allocation failure.
// The size of |a| and |b| are assumed to be public.
OPENSSL_EXPORT int BN_usub(BIGNUM *r, const BIGNUM *a, const BIGNUM *b);

// BN_sub_word subtracts |w| from |a|. It returns one on success and zero on
// allocation failure.
// The size of |a| and |w| are assumed to be public.
OPENSSL_EXPORT int BN_sub_word(BIGNUM *a, BN_ULONG w);

// BN_mul sets |r| = |a| * |b|, where |r| may be the same pointer as |a| or
// |b|. Returns one on success and zero otherwise.
// The size of |a| and |b| are assumed to be public.
OPENSSL_EXPORT int BN_mul(BIGNUM *r, const BIGNUM *a, const BIGNUM *b,
                          BN_CTX *ctx);

// BN_mul_word sets |bn| = |bn| * |w|. It returns one on success or zero on
// allocation failure.
// The size of |bn| and |w| are assumed to be public.
OPENSSL_EXPORT int BN_mul_word(BIGNUM *bn, BN_ULONG w);

// BN_sqr sets |r| = |a|^2 (i.e. squares), where |r| may be the same pointer as
// |a|. Returns one on success and zero otherwise. This is more efficient than
// BN_mul(r, a, a, ctx).
// The size of |a| is assumed to be public.
OPENSSL_EXPORT int BN_sqr(BIGNUM *r, const BIGNUM *a, BN_CTX *ctx);

// BN_div divides |numerator| by |divisor| and places the result in |quotient|
// and the remainder in |rem|. Either of |quotient| or |rem| may be NULL, in
// which case the respective value is not returned. The result is rounded
// towards zero; thus if |numerator| is negative, the remainder will be zero or
// negative. It returns one on success or zero on error.
// The size of arguments are assumed to be public.
OPENSSL_EXPORT int BN_div(BIGNUM *quotient, BIGNUM *rem,
                          const BIGNUM *numerator, const BIGNUM *divisor,
                          BN_CTX *ctx);

// BN_div_word sets |numerator| = |numerator|/|divisor| and returns the
// remainder or (BN_ULONG)-1 on error.
// The size of arguments are assumed to be public.
OPENSSL_EXPORT BN_ULONG BN_div_word(BIGNUM *numerator, BN_ULONG divisor);

// BN_sqrt sets |*out_sqrt| (which may be the same |BIGNUM| as |in|) to the
// square root of |in|, using |ctx|. It returns one on success or zero on
// error. Negative numbers and non-square numbers will result in an error with
// appropriate errors on the error queue.
// the size of |in| is assumed public.
OPENSSL_EXPORT int BN_sqrt(BIGNUM *out_sqrt, const BIGNUM *in, BN_CTX *ctx);


// Comparison functions

// BN_cmp returns a value less than, equal to or greater than zero if |a| is
// less than, equal to or greater than |b|, respectively.
OPENSSL_EXPORT int BN_cmp(const BIGNUM *a, const BIGNUM *b);

// BN_cmp_word is like |BN_cmp| except it takes its second argument as a
// |BN_ULONG| instead of a |BIGNUM|.
OPENSSL_EXPORT int BN_cmp_word(const BIGNUM *a, BN_ULONG b);

// BN_ucmp returns a value less than, equal to or greater than zero if the
// absolute value of |a| is less than, equal to or greater than the absolute
// value of |b|, respectively.
OPENSSL_EXPORT int BN_ucmp(const BIGNUM *a, const BIGNUM *b);

// BN_equal_consttime returns one if |a| is equal to |b|, and zero otherwise.
// It takes an amount of time dependent on the sizes of |a| and |b|, but
// independent of the contents (including the signs) of |a| and |b|.
OPENSSL_EXPORT int BN_equal_consttime(const BIGNUM *a, const BIGNUM *b);

// BN_abs_is_word returns one if the absolute value of |bn| equals |w| and zero
// otherwise.
OPENSSL_EXPORT int BN_abs_is_word(const BIGNUM *bn, BN_ULONG w);

// BN_is_zero returns one if |bn| is zero and zero otherwise.
OPENSSL_EXPORT int BN_is_zero(const BIGNUM *bn);

// BN_is_one returns one if |bn| equals one and zero otherwise.
OPENSSL_EXPORT int BN_is_one(const BIGNUM *bn);

// BN_is_word returns one if |bn| is exactly |w| and zero otherwise.
OPENSSL_EXPORT int BN_is_word(const BIGNUM *bn, BN_ULONG w);

// BN_is_odd returns one if |bn| is odd and zero otherwise.
OPENSSL_EXPORT int BN_is_odd(const BIGNUM *bn);

// BN_is_pow2 returns 1 if |a| is a power of two, and 0 otherwise.
OPENSSL_EXPORT int BN_is_pow2(const BIGNUM *a);


// Bitwise operations.

// BN_lshift sets |r| equal to |a| << n. The |a| and |r| arguments may be the
// same |BIGNUM|. It returns one on success and zero on allocation failure.
OPENSSL_EXPORT int BN_lshift(BIGNUM *r, const BIGNUM *a, int n);

// BN_lshift1 sets |r| equal to |a| << 1, where |r| and |a| may be the same
// pointer. It returns one on success and zero on allocation failure.
OPENSSL_EXPORT int BN_lshift1(BIGNUM *r, const BIGNUM *a);

// BN_rshift sets |r| equal to |a| >> n, where |r| and |a| may be the same
// pointer. It returns one on success and zero on allocation failure.
OPENSSL_EXPORT int BN_rshift(BIGNUM *r, const BIGNUM *a, int n);

// BN_rshift1 sets |r| equal to |a| >> 1, where |r| and |a| may be the same
// pointer. It returns one on success and zero on allocation failure.
OPENSSL_EXPORT int BN_rshift1(BIGNUM *r, const BIGNUM *a);

// BN_set_bit sets the |n|th, least-significant bit in |a|. For example, if |a|
// is 2 then setting bit zero will make it 3. It returns one on success or zero
// on allocation failure.
OPENSSL_EXPORT int BN_set_bit(BIGNUM *a, int n);

// BN_clear_bit clears the |n|th, least-significant bit in |a|. For example, if
// |a| is 3, clearing bit zero will make it two. It returns one on success or
// zero on allocation failure.
OPENSSL_EXPORT int BN_clear_bit(BIGNUM *a, int n);

// BN_is_bit_set returns one if the |n|th least-significant bit in |a| exists
// and is set. Otherwise, it returns zero.
OPENSSL_EXPORT int BN_is_bit_set(const BIGNUM *a, int n);

// BN_mask_bits truncates |a| so that it is only |n| bits long. It returns one
// on success or zero if |n| is negative.
//
// This differs from OpenSSL which additionally returns zero if |a|'s word
// length is less than or equal to |n|, rounded down to a number of words. Note
// word size is platform-dependent, so this behavior is also difficult to rely
// on in OpenSSL and not very useful.
OPENSSL_EXPORT int BN_mask_bits(BIGNUM *a, int n);

// BN_count_low_zero_bits returns the number of low-order zero bits in |bn|, or
// the number of factors of two which divide it. It returns zero if |bn| is
// zero.
OPENSSL_EXPORT int BN_count_low_zero_bits(const BIGNUM *bn);


// Modulo arithmetic.

// BN_mod_word returns |a| mod |w| or (BN_ULONG)-1 on error.
OPENSSL_EXPORT BN_ULONG BN_mod_word(const BIGNUM *a, BN_ULONG w);

// BN_mod_pow2 sets |r| = |a| mod 2^|e|. It returns 1 on success and
// 0 on error.
OPENSSL_EXPORT int BN_mod_pow2(BIGNUM *r, const BIGNUM *a, size_t e);

// BN_nnmod_pow2 sets |r| = |a| mod 2^|e| where |r| is always positive.
// It returns 1 on success and 0 on error.
OPENSSL_EXPORT int BN_nnmod_pow2(BIGNUM *r, const BIGNUM *a, size_t e);

// BN_mod is a helper macro that calls |BN_div| and discards the quotient.
#define BN_mod(rem, numerator, divisor, ctx) \
  BN_div(NULL, (rem), (numerator), (divisor), (ctx))

// BN_nnmod is a non-negative modulo function. It acts like |BN_mod|, but 0 <=
// |rem| < |divisor| is always true. It returns one on success and zero on
// error.
OPENSSL_EXPORT int BN_nnmod(BIGNUM *rem, const BIGNUM *numerator,
                            const BIGNUM *divisor, BN_CTX *ctx);

// BN_mod_add sets |r| = |a| + |b| mod |m|. It returns one on success and zero
// on error.
OPENSSL_EXPORT int BN_mod_add(BIGNUM *r, const BIGNUM *a, const BIGNUM *b,
                              const BIGNUM *m, BN_CTX *ctx);

// BN_mod_add_quick acts like |BN_mod_add| but requires that |a| and |b| be
// non-negative and less than |m|.
OPENSSL_EXPORT int BN_mod_add_quick(BIGNUM *r, const BIGNUM *a, const BIGNUM *b,
                                    const BIGNUM *m);

// BN_mod_sub sets |r| = |a| - |b| mod |m|. It returns one on success and zero
// on error.
OPENSSL_EXPORT int BN_mod_sub(BIGNUM *r, const BIGNUM *a, const BIGNUM *b,
                              const BIGNUM *m, BN_CTX *ctx);

// BN_mod_sub_quick acts like |BN_mod_sub| but requires that |a| and |b| be
// non-negative and less than |m|.
OPENSSL_EXPORT int BN_mod_sub_quick(BIGNUM *r, const BIGNUM *a, const BIGNUM *b,
                                    const BIGNUM *m);

// BN_mod_mul sets |r| = |a|*|b| mod |m|. It returns one on success and zero
// on error.
OPENSSL_EXPORT int BN_mod_mul(BIGNUM *r, const BIGNUM *a, const BIGNUM *b,
                              const BIGNUM *m, BN_CTX *ctx);

// BN_mod_sqr sets |r| = |a|^2 mod |m|. It returns one on success and zero
// on error.
OPENSSL_EXPORT int BN_mod_sqr(BIGNUM *r, const BIGNUM *a, const BIGNUM *m,
                              BN_CTX *ctx);

// BN_mod_lshift sets |r| = (|a| << n) mod |m|, where |r| and |a| may be the
// same pointer. It returns one on success and zero on error.
OPENSSL_EXPORT int BN_mod_lshift(BIGNUM *r, const BIGNUM *a, int n,
                                 const BIGNUM *m, BN_CTX *ctx);

// BN_mod_lshift_quick acts like |BN_mod_lshift| but requires that |a| be
// non-negative and less than |m|.
OPENSSL_EXPORT int BN_mod_lshift_quick(BIGNUM *r, const BIGNUM *a, int n,
                                       const BIGNUM *m);

// BN_mod_lshift1 sets |r| = (|a| << 1) mod |m|, where |r| and |a| may be the
// same pointer. It returns one on success and zero on error.
OPENSSL_EXPORT int BN_mod_lshift1(BIGNUM *r, const BIGNUM *a, const BIGNUM *m,
                                  BN_CTX *ctx);

// BN_mod_lshift1_quick acts like |BN_mod_lshift1| but requires that |a| be
// non-negative and less than |m|.
OPENSSL_EXPORT int BN_mod_lshift1_quick(BIGNUM *r, const BIGNUM *a,
                                        const BIGNUM *m);

// BN_mod_sqrt returns a newly-allocated |BIGNUM|, r, such that
// r^2 == a (mod p). It returns NULL on error or if |a| is not a square mod |p|.
// In the latter case, it will add |BN_R_NOT_A_SQUARE| to the error queue.
// If |a| is a square and |p| > 2, there are two possible square roots. This
// function may return either and may even select one non-deterministically.
//
// This function only works if |p| is a prime. If |p| is composite, it may fail
// or return an arbitrary value. Callers should not pass attacker-controlled
// values of |p|.
OPENSSL_EXPORT BIGNUM *BN_mod_sqrt(BIGNUM *in, const BIGNUM *a, const BIGNUM *p,
                                   BN_CTX *ctx);


// Random and prime number generation.

// The following are values for the |top| parameter of |BN_rand|.
#define BN_RAND_TOP_ANY    (-1)
#define BN_RAND_TOP_ONE     0
#define BN_RAND_TOP_TWO     1

// The following are values for the |bottom| parameter of |BN_rand|.
#define BN_RAND_BOTTOM_ANY  0
#define BN_RAND_BOTTOM_ODD  1

// BN_rand sets |rnd| to a random number of length |bits|. It returns one on
// success and zero otherwise.
//
// |top| must be one of the |BN_RAND_TOP_*| values. If |BN_RAND_TOP_ONE|, the
// most-significant bit, if any, will be set. If |BN_RAND_TOP_TWO|, the two
// most significant bits, if any, will be set. If |BN_RAND_TOP_ANY|, no extra
// action will be taken and |BN_num_bits(rnd)| may not equal |bits| if the most
// significant bits randomly ended up as zeros.
//
// |bottom| must be one of the |BN_RAND_BOTTOM_*| values. If
// |BN_RAND_BOTTOM_ODD|, the least-significant bit, if any, will be set. If
// |BN_RAND_BOTTOM_ANY|, no extra action will be taken.
OPENSSL_EXPORT int BN_rand(BIGNUM *rnd, int bits, int top, int bottom);

// BN_pseudo_rand is an alias for |BN_rand|.
OPENSSL_EXPORT int BN_pseudo_rand(BIGNUM *rnd, int bits, int top, int bottom);

// BN_rand_range is equivalent to |BN_rand_range_ex| with |min_inclusive| set
// to zero and |max_exclusive| set to |range|.
OPENSSL_EXPORT int BN_rand_range(BIGNUM *rnd, const BIGNUM *range);

// BN_rand_range_ex sets |rnd| to a random value in
// [min_inclusive..max_exclusive). It returns one on success and zero
// otherwise.
OPENSSL_EXPORT int BN_rand_range_ex(BIGNUM *r, BN_ULONG min_inclusive,
                                    const BIGNUM *max_exclusive);

// BN_pseudo_rand_range is an alias for BN_rand_range.
OPENSSL_EXPORT int BN_pseudo_rand_range(BIGNUM *rnd, const BIGNUM *range);

#define BN_GENCB_GENERATED 0
#define BN_GENCB_PRIME_TEST 1

// bn_gencb_st, or |BN_GENCB|, holds a callback function that is used by
// generation functions that can take a very long time to complete. Use
// |BN_GENCB_set| to initialise a |BN_GENCB| structure.
//
// The callback receives the address of that |BN_GENCB| structure as its last
// argument and the user is free to put an arbitrary pointer in |arg|. The other
// arguments are set as follows:
// - event=BN_GENCB_GENERATED, n=i:   after generating the i'th possible prime
//                                    number.
// - event=BN_GENCB_PRIME_TEST, n=-1: when finished trial division primality
//                                    checks.
// - event=BN_GENCB_PRIME_TEST, n=i:  when the i'th primality test has finished.
//
// The callback can return zero to abort the generation progress or one to
// allow it to continue.
//
// When other code needs to call a BN generation function it will often take a
// BN_GENCB argument and may call the function with other argument values.
struct bn_gencb_st {
  uint8_t type;
  void *arg;        // callback-specific data
  union {
    int (*new_style)(int event, int n, struct bn_gencb_st *);
    void (*old_style)(int, int, void *);
  } callback;
};

// BN_GENCB_new returns a newly-allocated |BN_GENCB| object, or NULL on
// allocation failure. The result must be released with |BN_GENCB_free| when
// done.
OPENSSL_EXPORT BN_GENCB *BN_GENCB_new(void);

// BN_GENCB_free releases memory associated with |callback|.
OPENSSL_EXPORT void BN_GENCB_free(BN_GENCB *callback);

// BN_GENCB_set configures |callback| to call |f| and sets |callback->arg| to
// |arg|. |BN_GENCB_set| is recommended over |BN_GENCB_set_old| as |BN_GENCB_set|
// accepts callbacks that return a result and have a strong type for the
// |BN_GENCB|. Only one callback can be configured in a |BN_GENCB|, calling
// |BN_GENCB_set| or |BN_GENCB_set_old| multiple times will overwrite the
// callback.
OPENSSL_EXPORT void BN_GENCB_set(BN_GENCB *callback,
                                 int (*f)(int event, int n, BN_GENCB *),
                                 void *arg);

// BN_GENCB_call calls |callback|, if not NULL, and returns the return value of
// the callback, or 1 if |callback| is NULL.
OPENSSL_EXPORT int BN_GENCB_call(BN_GENCB *callback, int event, int n);

// BN_GENCB_get_arg returns |callback->arg|.
OPENSSL_EXPORT void *BN_GENCB_get_arg(const BN_GENCB *callback);

// BN_generate_prime_ex sets |ret| to a prime number of |bits| length. If safe
// is non-zero then the prime will be such that (ret-1)/2 is also a prime.
// (This is needed for Diffie-Hellman groups to ensure that the only subgroups
// are of size 2 and (p-1)/2.).
//
// If |add| is not NULL, the prime will fulfill the condition |ret| % |add| ==
// |rem| in order to suit a given generator. (If |rem| is NULL then |ret| %
// |add| == 1.)
//
// If |cb| is not NULL, it will be called during processing to give an
// indication of progress. See the comments for |BN_GENCB|. It returns one on
// success and zero otherwise.
OPENSSL_EXPORT int BN_generate_prime_ex(BIGNUM *ret, int bits, int safe,
                                        const BIGNUM *add, const BIGNUM *rem,
                                        BN_GENCB *cb);

// BN_prime_checks_for_validation can be used as the |checks| argument to the
// primarily testing functions when validating an externally-supplied candidate
// prime. It gives a false positive rate of at most 2^{-128}. (The worst case
// false positive rate for a single iteration is 1/4 per
// https://eprint.iacr.org/2018/749. (1/4)^64 = 2^{-128}.)
#define BN_prime_checks_for_validation 64

// BN_prime_checks_for_generation can be used as the |checks| argument to the
// primality testing functions when generating random primes. It gives a false
// positive rate at most the security level of the corresponding RSA key size.
//
// Note this value only performs enough checks if the candidate prime was
// selected randomly. If validating an externally-supplied candidate, especially
// one that may be selected adversarially, use |BN_prime_checks_for_validation|
// instead.
#define BN_prime_checks_for_generation 0

// bn_primality_result_t enumerates the outcomes of primality-testing.
enum bn_primality_result_t {
  bn_probably_prime,
  bn_composite,
  bn_non_prime_power_composite
};

// BN_enhanced_miller_rabin_primality_test tests whether |w| is probably a prime
// number using the Enhanced Miller-Rabin Test (FIPS 186-4 C.3.2) with
// |checks| iterations and returns the result in |out_result|. Enhanced
// Miller-Rabin tests primality for odd integers greater than 3, returning
// |bn_probably_prime| if the number is probably prime,
// |bn_non_prime_power_composite| if the number is a composite that is not the
// power of a single prime, and |bn_composite| otherwise. It returns one on
// success and zero on failure. If |cb| is not NULL, then it is called during
// each iteration of the primality test.
//
// See |BN_prime_checks_for_validation| and |BN_prime_checks_for_generation| for
// recommended values of |checks|.
OPENSSL_EXPORT int BN_enhanced_miller_rabin_primality_test(
    enum bn_primality_result_t *out_result, const BIGNUM *w, int checks,
    BN_CTX *ctx, BN_GENCB *cb);

// BN_primality_test sets |*is_probably_prime| to one if |candidate| is
// probably a prime number by the Miller-Rabin test or zero if it's certainly
// not.
//
// If |do_trial_division| is non-zero then |candidate| will be tested against a
// list of small primes before Miller-Rabin tests. The probability of this
// function returning a false positive is at most 2^{2*checks}. See
// |BN_prime_checks_for_validation| and |BN_prime_checks_for_generation| for
// recommended values of |checks|.
//
// If |cb| is not NULL then it is called during the checking process. See the
// comment above |BN_GENCB|.
//
// The function returns one on success and zero on error.
OPENSSL_EXPORT int BN_primality_test(int *is_probably_prime,
                                     const BIGNUM *candidate, int checks,
                                     BN_CTX *ctx, int do_trial_division,
                                     BN_GENCB *cb);

// BN_is_prime_fasttest_ex returns one if |candidate| is probably a prime
// number by the Miller-Rabin test, zero if it's certainly not and -1 on error.
//
// If |do_trial_division| is non-zero then |candidate| will be tested against a
// list of small primes before Miller-Rabin tests. The probability of this
// function returning one when |candidate| is composite is at most 2^{2*checks}.
// See |BN_prime_checks_for_validation| and |BN_prime_checks_for_generation| for
// recommended values of |checks|.
//
// If |cb| is not NULL then it is called during the checking process. See the
// comment above |BN_GENCB|.
//
// WARNING: deprecated. Use |BN_primality_test|.
OPENSSL_EXPORT int BN_is_prime_fasttest_ex(const BIGNUM *candidate, int checks,
                                           BN_CTX *ctx, int do_trial_division,
                                           BN_GENCB *cb);

// BN_is_prime_ex acts the same as |BN_is_prime_fasttest_ex| with
// |do_trial_division| set to zero.
//
// WARNING: deprecated: Use |BN_primality_test|.
OPENSSL_EXPORT int BN_is_prime_ex(const BIGNUM *candidate, int checks,
                                  BN_CTX *ctx, BN_GENCB *cb);


// Number theory functions

// BN_gcd sets |r| = gcd(|a|, |b|). It returns one on success and zero
// otherwise.
OPENSSL_EXPORT int BN_gcd(BIGNUM *r, const BIGNUM *a, const BIGNUM *b,
                          BN_CTX *ctx);

// BN_mod_inverse sets |out| equal to |a|^-1, mod |n|. If |out| is NULL, a
// fresh BIGNUM is allocated. It returns the result or NULL on error.
//
// If |n| is even then the operation is performed using an algorithm that avoids
// some branches but which isn't constant-time. This function shouldn't be used
// for secret values; use |BN_mod_inverse_blinded| instead. Or, if |n| is
// guaranteed to be prime, use
// |BN_mod_exp_mont_consttime(out, a, m_minus_2, m, ctx, m_mont)|, taking
// advantage of Fermat's Little Theorem.
OPENSSL_EXPORT BIGNUM *BN_mod_inverse(BIGNUM *out, const BIGNUM *a,
                                      const BIGNUM *n, BN_CTX *ctx);

// BN_mod_inverse_blinded sets |out| equal to |a|^-1, mod |n|, where |n| is the
// Montgomery modulus for |mont|. |a| must be non-negative and must be less
// than |n|. |n| must be greater than 1. |a| is blinded (masked by a random
// value) to protect it against side-channel attacks. On failure, if the failure
// was caused by |a| having no inverse mod |n| then |*out_no_inverse| will be
// set to one; otherwise it will be set to zero.
//
// Note this function may incorrectly report |a| has no inverse if the random
// blinding value has no inverse. It should only be used when |n| has few
// non-invertible elements, such as an RSA modulus.
OPENSSL_EXPORT int BN_mod_inverse_blinded(BIGNUM *out, int *out_no_inverse,
                                          const BIGNUM *a,
                                          const BN_MONT_CTX *mont, BN_CTX *ctx);

// BN_mod_inverse_odd sets |out| equal to |a|^-1, mod |n|. |a| must be
// non-negative and must be less than |n|. |n| must be odd. This function
// shouldn't be used for secret values; use |BN_mod_inverse_blinded| instead.
// Or, if |n| is guaranteed to be prime, use
// |BN_mod_exp_mont_consttime(out, a, m_minus_2, m, ctx, m_mont)|, taking
// advantage of Fermat's Little Theorem. It returns one on success or zero on
// failure. On failure, if the failure was caused by |a| having no inverse mod
// |n| then |*out_no_inverse| will be set to one; otherwise it will be set to
// zero.
int BN_mod_inverse_odd(BIGNUM *out, int *out_no_inverse, const BIGNUM *a,
                       const BIGNUM *n, BN_CTX *ctx);


// Montgomery arithmetic.

// BN_MONT_CTX contains the precomputed values needed to work in a specific
// Montgomery domain.

// BN_MONT_CTX_new_for_modulus returns a fresh |BN_MONT_CTX| given the modulus,
// |mod| or NULL on error. Note this function assumes |mod| is public.
OPENSSL_EXPORT BN_MONT_CTX *BN_MONT_CTX_new_for_modulus(const BIGNUM *mod,
                                                        BN_CTX *ctx);

// BN_MONT_CTX_new_consttime behaves like |BN_MONT_CTX_new_for_modulus| but
// treats |mod| as secret.
OPENSSL_EXPORT BN_MONT_CTX *BN_MONT_CTX_new_consttime(const BIGNUM *mod,
                                                      BN_CTX *ctx);

// BN_MONT_CTX_free frees memory associated with |mont|.
OPENSSL_EXPORT void BN_MONT_CTX_free(BN_MONT_CTX *mont);

// BN_MONT_CTX_copy sets |to| equal to |from|. It returns |to| on success or
// NULL on error.
OPENSSL_EXPORT BN_MONT_CTX *BN_MONT_CTX_copy(BN_MONT_CTX *to,
                                             const BN_MONT_CTX *from);

// BN_to_montgomery sets |ret| equal to |a| in the Montgomery domain. |a| is
// assumed to be in the range [0, n), where |n| is the Montgomery modulus. It
// returns one on success or zero on error.
OPENSSL_EXPORT int BN_to_montgomery(BIGNUM *ret, const BIGNUM *a,
                                    const BN_MONT_CTX *mont, BN_CTX *ctx);

// BN_from_montgomery sets |ret| equal to |a| * R^-1, i.e. translates values out
// of the Montgomery domain. |a| is assumed to be in the range [0, n*R), where
// |n| is the Montgomery modulus. Note n < R, so inputs in the range [0, n*n)
// are valid. This function returns one on success or zero on error.
OPENSSL_EXPORT int BN_from_montgomery(BIGNUM *ret, const BIGNUM *a,
                                      const BN_MONT_CTX *mont, BN_CTX *ctx);

// BN_mod_mul_montgomery set |r| equal to |a| * |b|, in the Montgomery domain.
// Both |a| and |b| must already be in the Montgomery domain (by
// |BN_to_montgomery|). In particular, |a| and |b| are assumed to be in the
// range [0, n), where |n| is the Montgomery modulus. It returns one on success
// or zero on error.
OPENSSL_EXPORT int BN_mod_mul_montgomery(BIGNUM *r, const BIGNUM *a,
                                         const BIGNUM *b,
                                         const BN_MONT_CTX *mont, BN_CTX *ctx);


// Exponentiation.

// BN_exp sets |r| equal to |a|^{|p|}. It does so with a square-and-multiply
// algorithm that leaks side-channel information. It returns one on success or
// zero otherwise.
OPENSSL_EXPORT int BN_exp(BIGNUM *r, const BIGNUM *a, const BIGNUM *p,
                          BN_CTX *ctx);

// BN_mod_exp sets |r| equal to |a|^{|p|} mod |m|. It does so with the best
// algorithm for the values provided. It returns one on success or zero
// otherwise. The |BN_mod_exp_mont_consttime| variant must be used if the
// exponent is secret.
OPENSSL_EXPORT int BN_mod_exp(BIGNUM *r, const BIGNUM *a, const BIGNUM *p,
                              const BIGNUM *m, BN_CTX *ctx);

// BN_mod_exp_mont behaves like |BN_mod_exp| but treats |a| as secret and
// requires 0 <= |a| < |m|.
OPENSSL_EXPORT int BN_mod_exp_mont(BIGNUM *r, const BIGNUM *a, const BIGNUM *p,
                                   const BIGNUM *m, BN_CTX *ctx,
                                   const BN_MONT_CTX *mont);

// BN_mod_exp_mont_consttime behaves like |BN_mod_exp| but treats |a|, |p|, and
// |m| as secret and requires 0 <= |a| < |m|.
OPENSSL_EXPORT int BN_mod_exp_mont_consttime(BIGNUM *rr, const BIGNUM *a,
                                             const BIGNUM *p, const BIGNUM *m,
                                             BN_CTX *ctx,
                                             const BN_MONT_CTX *mont);


// Deprecated functions

// BN_GENCB_set_old behaves like |BN_GENCB_set| which is the recommended way to
// set a callback on a |BN_GENCB|. |BN_GENCB_set_old| configures |callback| to
// call |f| and sets |callback->arg| to |arg|. The only difference between
// |BN_GENCB_set| and |BN_GENCB_set_old| is the argument and return types in
// |callback|. Only one callback can be configured in a |BN_GENCB|, calling
// |BN_GENCB_set| or |BN_GENCB_set_old| multiple times will overwrite the
// callback.
OPENSSL_EXPORT void BN_GENCB_set_old(BN_GENCB *callback,
                                     void (*f)(int, int, void *), void *cb_arg);

// BN_bn2mpi serialises the value of |in| to |out|, using a format that consists
// of the number's length in bytes represented as a 4-byte big-endian number,
// and the number itself in big-endian format, where the most significant bit
// signals a negative number. (The representation of numbers with the MSB set is
// prefixed with null byte). |out| must have sufficient space available; to
// find the needed amount of space, call the function with |out| set to NULL.
OPENSSL_EXPORT size_t BN_bn2mpi(const BIGNUM *in, uint8_t *out);

// BN_mpi2bn parses |len| bytes from |in| and returns the resulting value. The
// bytes at |in| are expected to be in the format emitted by |BN_bn2mpi|.
//
// If |out| is NULL then a fresh |BIGNUM| is allocated and returned, otherwise
// |out| is reused and returned. On error, NULL is returned and the error queue
// is updated.
OPENSSL_EXPORT BIGNUM *BN_mpi2bn(const uint8_t *in, size_t len, BIGNUM *out);

// BN_mod_exp_mont_word is like |BN_mod_exp_mont| except that the base |a| is
// given as a |BN_ULONG| instead of a |BIGNUM *|. It returns one on success
// or zero otherwise.
OPENSSL_EXPORT int BN_mod_exp_mont_word(BIGNUM *r, BN_ULONG a, const BIGNUM *p,
                                        const BIGNUM *m, BN_CTX *ctx,
                                        const BN_MONT_CTX *mont);

// BN_mod_exp2_mont calculates (a1^p1) * (a2^p2) mod m. It returns 1 on success
// or zero otherwise.
OPENSSL_EXPORT int BN_mod_exp2_mont(BIGNUM *r, const BIGNUM *a1,
                                    const BIGNUM *p1, const BIGNUM *a2,
                                    const BIGNUM *p2, const BIGNUM *m,
                                    BN_CTX *ctx, const BN_MONT_CTX *mont);

// BN_MONT_CTX_new returns a fresh |BN_MONT_CTX| or NULL on allocation failure.
// Use |BN_MONT_CTX_new_for_modulus| instead.
OPENSSL_EXPORT BN_MONT_CTX *BN_MONT_CTX_new(void);

// BN_MONT_CTX_set sets up a Montgomery context given the modulus, |mod|. It
// returns one on success and zero on error. Use |BN_MONT_CTX_new_for_modulus|
// instead.
OPENSSL_EXPORT int BN_MONT_CTX_set(BN_MONT_CTX *mont, const BIGNUM *mod,
                                   BN_CTX *ctx);

// BN_bn2binpad behaves like |BN_bn2bin_padded|, but it returns |len| on success
// and -1 on error.
//
// Use |BN_bn2bin_padded| instead. It is |size_t|-clean.
OPENSSL_EXPORT int BN_bn2binpad(const BIGNUM *in, uint8_t *out, int len);

// BN_prime_checks is a deprecated alias for |BN_prime_checks_for_validation|.
// Use |BN_prime_checks_for_generation| or |BN_prime_checks_for_validation|
// instead. (This defaults to the |_for_validation| value in order to be
// conservative.)
#define BN_prime_checks BN_prime_checks_for_validation

// BN_secure_new calls |BN_new|.
OPENSSL_EXPORT BIGNUM *BN_secure_new(void);

// BN_CTX_secure_new calls |BN_CTX_new|.
OPENSSL_EXPORT BN_CTX *BN_CTX_secure_new(void);

// BN_mod_exp_mont_consttime_x2 performs two montgomery
// multiplications at once using AVX-512 SIMD. If AVX-512 is not
// available, it falls back to two calls of
// `BN_mod_exp_mont_consttime`.
OPENSSL_EXPORT int BN_mod_exp_mont_consttime_x2(BIGNUM *rr1, const BIGNUM *a1, const BIGNUM *p1,
                                                const BIGNUM *m1, const BN_MONT_CTX *in_mont1,
                                                BIGNUM *rr2, const BIGNUM *a2, const BIGNUM *p2,
                                                const BIGNUM *m2, const BN_MONT_CTX *in_mont2,
                                                BN_CTX *ctx);

// BN_set_flags does nothing. See comments regarding |BN_FLG_CONSTTIME| being
// intentionally omitted for more details.
OPENSSL_DEPRECATED OPENSSL_EXPORT void BN_set_flags(BIGNUM *b, int n);


// Private functions

struct bignum_st {
  // d is a pointer to an array of |width| |BN_BITS2|-bit chunks in
  // little-endian order. This stores the absolute value of the number.
  BN_ULONG *d;
  // width is the number of elements of |d| which are valid. This value is not
  // necessarily minimal; the most-significant words of |d| may be zero.
  // |width| determines a potentially loose upper-bound on the absolute value
  // of the |BIGNUM|.
  //
  // Functions taking |BIGNUM| inputs must compute the same answer for all
  // possible widths. |bn_minimal_width|, |bn_set_minimal_width|, and other
  // helpers may be used to recover the minimal width, provided it is not
  // secret. If it is secret, use a different algorithm. Functions may output
  // minimal or non-minimal |BIGNUM|s depending on secrecy requirements, but
  // those which cause widths to unboundedly grow beyond the minimal value
  // should be documented such.
  //
  // Note this is different from historical |BIGNUM| semantics.
  int width;
  // dmax is number of elements of |d| which are allocated.
  int dmax;
  // neg is one if the number if negative and zero otherwise.
  int neg;
  // flags is a bitmask of |BN_FLG_*| values
  int flags;
};

struct bn_mont_ctx_st {
  // RR is R^2, reduced modulo |N|. It is used to convert to Montgomery form. It
  // is guaranteed to have the same width as |N|.
  BIGNUM RR;
  // N is the modulus. It is always stored in minimal form, so |N.width|
  // determines R.
  BIGNUM N;
  BN_ULONG n0[2];  // least significant words of (R*Ri-1)/N
};

OPENSSL_EXPORT unsigned BN_num_bits_word(BN_ULONG l);

#define BN_FLG_MALLOCED 0x01
#define BN_FLG_STATIC_DATA 0x02

// |BN_FLG_CONSTTIME| has been removed and intentionally omitted so code relying
// on it will not compile unless the flag above is set. Consumers should use the
// higher-level cryptographic algorithms exposed by other modules. Consumers
// within the library should call the appropriate timing-sensitive algorithm
// directly.


#if defined(__cplusplus)
}  // extern C

#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(BIGNUM, BN_free)
BORINGSSL_MAKE_DELETER(BN_CTX, BN_CTX_free)
BORINGSSL_MAKE_DELETER(BN_MONT_CTX, BN_MONT_CTX_free)

class BN_CTXScope {
 public:
  BN_CTXScope(BN_CTX *ctx) : ctx_(ctx) { BN_CTX_start(ctx_); }
  ~BN_CTXScope() { BN_CTX_end(ctx_); }

 private:
  BN_CTX *ctx_;

  BN_CTXScope(BN_CTXScope &) = delete;
  BN_CTXScope &operator=(BN_CTXScope &) = delete;
};

BSSL_NAMESPACE_END

}  // extern C++
#endif

#endif

#define BN_R_ARG2_LT_ARG3 100
#define BN_R_BAD_RECIPROCAL 101
#define BN_R_BIGNUM_TOO_LONG 102
#define BN_R_BITS_TOO_SMALL 103
#define BN_R_CALLED_WITH_EVEN_MODULUS 104
#define BN_R_DIV_BY_ZERO 105
#define BN_R_EXPAND_ON_STATIC_BIGNUM_DATA 106
#define BN_R_INPUT_NOT_REDUCED 107
#define BN_R_INVALID_RANGE 108
#define BN_R_NEGATIVE_NUMBER 109
#define BN_R_NOT_A_SQUARE 110
#define BN_R_NOT_INITIALIZED 111
#define BN_R_NO_INVERSE 112
#define BN_R_PRIVATE_KEY_TOO_LARGE 113
#define BN_R_P_IS_NOT_PRIME 114
#define BN_R_TOO_MANY_ITERATIONS 115
#define BN_R_TOO_MANY_TEMPORARY_VARIABLES 116
#define BN_R_BAD_ENCODING 117
#define BN_R_ENCODE_ERROR 118
#define BN_R_INVALID_INPUT 119

#define BN_F_BN_GENERATE_PRIME_EX 0

#endif  // OPENSSL_HEADER_BN_H
