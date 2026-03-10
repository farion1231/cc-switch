/* Copyright (c) 2014, Google Inc.
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

#ifndef OPENSSL_HEADER_HKDF_H
#define OPENSSL_HEADER_HKDF_H

#include <openssl/base.h>
#include <openssl/kdf.h>

#if defined(__cplusplus)
extern "C" {
#endif


// HKDF.


// HKDF computes HKDF (as specified by RFC 5869) of initial keying material
// |secret| with |salt| and |info| using |digest|, and outputs |out_len| bytes
// to |out_key|. It returns one on success and zero on error.
//
// HKDF is an Extract-and-Expand algorithm. It does not do any key stretching,
// and as such, is not suited to be used alone to generate a key from a
// password.
OPENSSL_EXPORT int HKDF(uint8_t *out_key, size_t out_len, const EVP_MD *digest,
                        const uint8_t *secret, size_t secret_len,
                        const uint8_t *salt, size_t salt_len,
                        const uint8_t *info, size_t info_len);

// HKDF_extract computes a HKDF PRK (as specified by RFC 5869) from initial
// keying material |secret| and salt |salt| using |digest|, and outputs
// |out_len| bytes to |out_key|. The maximum output size is |EVP_MAX_MD_SIZE|.
// It returns one on success and zero on error.
//
// WARNING: This function orders the inputs differently from RFC 5869
// specification. Double-check which parameter is the secret/IKM and which is
// the salt when using.
OPENSSL_EXPORT int HKDF_extract(uint8_t *out_key, size_t *out_len,
                                const EVP_MD *digest, const uint8_t *secret,
                                size_t secret_len, const uint8_t *salt,
                                size_t salt_len);

// HKDF_expand computes a HKDF OKM (as specified by RFC 5869) of length
// |out_len| from the PRK |prk| and info |info| using |digest|, and outputs
// the result to |out_key|. It returns one on success and zero on error.
OPENSSL_EXPORT int HKDF_expand(uint8_t *out_key, size_t out_len,
                               const EVP_MD *digest, const uint8_t *prk,
                               size_t prk_len, const uint8_t *info,
                               size_t info_len);


#if defined(__cplusplus)
}  // extern C
#endif

#define HKDF_R_OUTPUT_TOO_LARGE 100

#endif  // OPENSSL_HEADER_HKDF_H
