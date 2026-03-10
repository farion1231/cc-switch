/* Copyright (c) 2022, Google Inc.
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

#ifndef OPENSSL_HEADER_CTRDRBG_H
#define OPENSSL_HEADER_CTRDRBG_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// FIPS pseudo-random number generator.


// CTR-DRBG state objects.
//
// CTR_DRBG_STATE contains the state of a FIPS AES-CTR-based pseudo-random
// number generator. If BoringSSL was built in FIPS mode then this is a FIPS
// Approved algorithm.

// CTR_DRBG_ENTROPY_LEN is the number of bytes of input entropy. See SP
// 800-90Ar1, table 3.
#define CTR_DRBG_ENTROPY_LEN 48

// CTR_DRBG_MAX_GENERATE_LENGTH is the maximum number of bytes that can be
// generated in a single call to |CTR_DRBG_generate|.
#define CTR_DRBG_MAX_GENERATE_LENGTH 65536

// CTR_DRBG_new returns an initialized |CTR_DRBG_STATE|, or NULL if either
// allocation failed or if |personalization_len| is invalid.
OPENSSL_EXPORT CTR_DRBG_STATE *CTR_DRBG_new(
    const uint8_t entropy[CTR_DRBG_ENTROPY_LEN], const uint8_t *personalization,
    size_t personalization_len);

// CTR_DRBG_free frees |state| if non-NULL, or else does nothing.
OPENSSL_EXPORT void CTR_DRBG_free(CTR_DRBG_STATE* state);

// CTR_DRBG_reseed reseeds |drbg| given |CTR_DRBG_ENTROPY_LEN| bytes of entropy
// in |entropy| and, optionally, up to |CTR_DRBG_ENTROPY_LEN| bytes of
// additional data. It returns one on success or zero on error. |entropy| and
// |additional_data| must not alias.
OPENSSL_EXPORT int CTR_DRBG_reseed(CTR_DRBG_STATE *drbg,
                                   const uint8_t entropy[CTR_DRBG_ENTROPY_LEN],
                                   const uint8_t *additional_data,
                                   size_t additional_data_len);

// CTR_DRBG_generate processes to up |CTR_DRBG_ENTROPY_LEN| bytes of additional
// data (if any) and then writes |out_len| random bytes to |out|, where
// |out_len| <= |CTR_DRBG_MAX_GENERATE_LENGTH|. It returns one on success or
// zero on error.
OPENSSL_EXPORT int CTR_DRBG_generate(CTR_DRBG_STATE *drbg, uint8_t *out,
                                     size_t out_len,
                                     const uint8_t *additional_data,
                                     size_t additional_data_len);

// CTR_DRBG_clear zeroises the state of |drbg|.
OPENSSL_EXPORT void CTR_DRBG_clear(CTR_DRBG_STATE *drbg);


#if defined(__cplusplus)
}  // extern C

extern "C++" {
BSSL_NAMESPACE_BEGIN
BORINGSSL_MAKE_DELETER(CTR_DRBG_STATE, CTR_DRBG_free)
BSSL_NAMESPACE_END
}  // extern C++
#endif

#endif  // OPENSSL_HEADER_CTRDRBG_H
