/* Copyright (c) 2021, Google Inc.
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

#ifndef OPENSSL_HEADER_BLAKE2_H
#define OPENSSL_HEADER_BLAKE2_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


#define BLAKE2B256_DIGEST_LENGTH (256 / 8)
#define BLAKE2B_CBLOCK 128

struct blake2b_state_st {
  uint64_t h[8];
  uint64_t t_low, t_high;
  uint8_t block[BLAKE2B_CBLOCK];
  size_t block_used;
};

// BLAKE2B256_Init initialises |b2b| to perform a BLAKE2b-256 hash. There are no
// pointers inside |b2b| thus release of |b2b| is purely managed by the caller.
OPENSSL_EXPORT void BLAKE2B256_Init(BLAKE2B_CTX *b2b);

// BLAKE2B256_Update appends |len| bytes from |data| to the digest being
// calculated by |b2b|.
OPENSSL_EXPORT void BLAKE2B256_Update(BLAKE2B_CTX *b2b, const void *data,
                                      size_t len);

// BLAKE2B256_Final completes the digest calculated by |b2b| and writes
// |BLAKE2B256_DIGEST_LENGTH| bytes to |out|.
OPENSSL_EXPORT void BLAKE2B256_Final(uint8_t out[BLAKE2B256_DIGEST_LENGTH],
                                     BLAKE2B_CTX *b2b);

// BLAKE2B256 writes the BLAKE2b-256 digset of |len| bytes from |data| to
// |out|.
OPENSSL_EXPORT void BLAKE2B256(const uint8_t *data, size_t len,
                               uint8_t out[BLAKE2B256_DIGEST_LENGTH]);


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_BLAKE2_H
