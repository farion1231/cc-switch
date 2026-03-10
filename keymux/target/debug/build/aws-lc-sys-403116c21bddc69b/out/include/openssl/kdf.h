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

#ifndef OPENSSL_HEADER_KDF_H
#define OPENSSL_HEADER_KDF_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif

// CRYPTO_tls1_prf calculates |out_len| bytes of the TLS PRF, using |digest|,
// and writes them to |out|. It returns one on success and zero on error.
// TLS 1.2: https://datatracker.ietf.org/doc/html/rfc5246#section-5
// TLS 1.{0,1}: https://datatracker.ietf.org/doc/html/rfc4346#section-5
OPENSSL_EXPORT int CRYPTO_tls1_prf(const EVP_MD *digest,
                                   uint8_t *out, size_t out_len,
                                   const uint8_t *secret, size_t secret_len,
                                   const char *label, size_t label_len,
                                   const uint8_t *seed1, size_t seed1_len,
                                   const uint8_t *seed2, size_t seed2_len);

// SSKDF_digest computes the One-step key derivation using the
// provided digest algorithm as the backing PRF. This algorithm
// may be referred to as "Single-Step KDF" or "NIST Concatenation KDF" by other
// implementors. |info_len| may be zero length.
//
// Returns a 1 on success, otherwise returns 0.
//
// This implementation adheres to the algorithm specified in Section 4 of the
// NIST Special Publication 800-56C Revision 2 published on August 2020. The
// parameters relevant to the specification are as follows:
// * Auxillary Function H is Option 1
// * |out_len|, |secret_len|, and |info_len| are specified in bytes
// * |out_len|, |secret_len|, |info_len| each must be <= 2^30
// * |out_len| and |secret_len| > 0
// * |out_len|, |secret_len| are analogous to |L| and |Z| respectively in the
// specification.
// * |info| and |info_len| refer to |FixedInfo| in the specification.
//
// Specification is available at https://doi.org/10.6028/NIST.SP.800-56Cr2
OPENSSL_EXPORT int SSKDF_digest(uint8_t *out_key, size_t out_len,
                                const EVP_MD *digest,
                                const uint8_t *secret, size_t secret_len,
                                const uint8_t *info, size_t info_len);

// SSKDF_hmac computes the One-step key derivation using the
// provided digest algorithm with HMAC as the backing PRF. This algorithm
// may be referred to as "Single-Step KDF" or "NIST Concatenation KDF" by other
// implementors. |salt| is optional and may be |NULL| or zero-length. In
// addition |info_len| may be zero length.
//
// Returns a 1 on success, otherwise returns 0.
//
// This implementation adheres to the algorithm specified in Section 4 of the
// NIST Special Publication 800-56C Revision 2 published on August 2020. The
// parameters relevant to the specification are as follows:
// * Auxillary Function H is Option 2
// * |out_len|, |secret_len|, |info_len|, and |salt_len| are specified in bytes
// * |out_len|, |secret_len|, |info_len| each must be <= 2^30
// * |out_len| and |secret_len| > 0
// * |out_len|, |secret_len| are analogous to |L| and |Z| respectively in the
// specification.
// * |info| and |info_len| refer to |FixedInfo| in the specification.
// * |salt| and |salt_len| refer to |salt| in the specification.
// * |salt| or |salt_len| being |NULL| or |0| respectively will result in a
//   default salt being used which will be an all-zero byte string whose length
//   is equal to the length of the specified |digest| input block length in
//   bytes.
OPENSSL_EXPORT int SSKDF_hmac(uint8_t *out_key, size_t out_len,
                              const EVP_MD *digest,
                              const uint8_t *secret, size_t secret_len,
                              const uint8_t *info, size_t info_len,
                              const uint8_t *salt, size_t salt_len);

// KBKDF_ctr_hmac derives keying material using the KDF counter mode algorithm,
// using the provided key derivation key |secret| and fixed info |info|.
// |info| or |info_len| may be zero-length. This algorithm
// may be referred to as a "Key-Based Key Derivation Function in Counter Mode".
//
// This implementation adheres to the algorithm specified in Section 4.1 of the
// NIST Special Publication 800-108 Revision 1 Update 1 published on August
// 2022. The parameters relevant to the specification are as follows:
// * |out_len|, |secret_len|, and |info_len| are specified in bytes
// * |out_len| is analogous to |L| in the specification.
// * |r| is the length of the binary representation of the counter |i|
//   referred to by the specification. |r| is 32 bits in this implementation.
// * The 32-bit counter is big-endian in this implementation.
// * The 32-bit counter location is placed before |info|.
// * |K_IN| is analogous to |secret| and |secret_len|.
// * |PRF| refers to HMAC in this implementation.
//
// Specification is available at https://doi.org/10.6028/NIST.SP.800-108r1-upd1
OPENSSL_EXPORT int KBKDF_ctr_hmac(uint8_t *out_key, size_t out_len,
                                  const EVP_MD *digest, const uint8_t *secret,
                                  size_t secret_len, const uint8_t *info,
                                  size_t info_len);

// KDF support for EVP.


// HKDF-specific functions.
//
// The following functions are provided for OpenSSL compatibility. Prefer the
// HKDF functions in <openssl/hkdf.h>. In each, |ctx| must be created with
// |EVP_PKEY_CTX_new_id| with |EVP_PKEY_HKDF| and then initialized with
// |EVP_PKEY_derive_init|.

// EVP_PKEY_HKDEF_MODE_* define "modes" for use with |EVP_PKEY_CTX_hkdf_mode|.
// The mispelling of "HKDF" as "HKDEF" is intentional for OpenSSL compatibility.
#define EVP_PKEY_HKDEF_MODE_EXTRACT_AND_EXPAND 0
#define EVP_PKEY_HKDEF_MODE_EXTRACT_ONLY 1
#define EVP_PKEY_HKDEF_MODE_EXPAND_ONLY 2

// EVP_PKEY_CTX_hkdf_mode configures which HKDF operation to run. It returns one
// on success and zero on error. |mode| must be one of |EVP_PKEY_HKDEF_MODE_*|.
// By default, the mode is |EVP_PKEY_HKDEF_MODE_EXTRACT_AND_EXPAND|.
//
// If |mode| is |EVP_PKEY_HKDEF_MODE_EXTRACT_AND_EXPAND| or
// |EVP_PKEY_HKDEF_MODE_EXPAND_ONLY|, the output is variable-length.
// |EVP_PKEY_derive| uses the size of the output buffer as the output length for
// HKDF-Expand.
//
// WARNING: Although this API calls it a "mode", HKDF-Extract and HKDF-Expand
// are distinct operations with distinct inputs and distinct kinds of keys.
// Callers should not pass input secrets for one operation into the other.
OPENSSL_EXPORT int EVP_PKEY_CTX_hkdf_mode(EVP_PKEY_CTX *ctx, int mode);

// EVP_PKEY_CTX_set_hkdf_md sets |md| as the digest to use with HKDF. It returns
// one on success and zero on error.
OPENSSL_EXPORT int EVP_PKEY_CTX_set_hkdf_md(EVP_PKEY_CTX *ctx,
                                            const EVP_MD *md);

// EVP_PKEY_CTX_set1_hkdf_key configures HKDF to use |key_len| bytes from |key|
// as the "key", described below. It returns one on success and zero on error.
//
// Which input is the key depends on the "mode" (see |EVP_PKEY_CTX_hkdf_mode|).
// If |EVP_PKEY_HKDEF_MODE_EXTRACT_AND_EXPAND| or
// |EVP_PKEY_HKDEF_MODE_EXTRACT_ONLY|, this function specifies the input keying
// material (IKM) for HKDF-Extract. If |EVP_PKEY_HKDEF_MODE_EXPAND_ONLY|, it
// instead specifies the pseudorandom key (PRK) for HKDF-Expand.
OPENSSL_EXPORT int EVP_PKEY_CTX_set1_hkdf_key(EVP_PKEY_CTX *ctx,
                                              const uint8_t *key,
                                              size_t key_len);

// EVP_PKEY_CTX_set1_hkdf_salt configures HKDF to use |salt_len| bytes from
// |salt| as the salt parameter to HKDF-Extract. It returns one on success and
// zero on error. If performing HKDF-Expand only, this parameter is ignored.
OPENSSL_EXPORT int EVP_PKEY_CTX_set1_hkdf_salt(EVP_PKEY_CTX *ctx,
                                               const uint8_t *salt,
                                               size_t salt_len);

// EVP_PKEY_CTX_add1_hkdf_info appends |info_len| bytes from |info| to the info
// parameter used with HKDF-Expand. It returns one on success and zero on error.
// If performing HKDF-Extract only, this parameter is ignored.
OPENSSL_EXPORT int EVP_PKEY_CTX_add1_hkdf_info(EVP_PKEY_CTX *ctx,
                                               const uint8_t *info,
                                               size_t info_len);


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_KDF_H
