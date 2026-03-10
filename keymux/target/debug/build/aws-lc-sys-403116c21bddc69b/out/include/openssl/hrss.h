/* Copyright (c) 2018, Google Inc.
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

#ifndef OPENSSL_HEADER_HRSS_H
#define OPENSSL_HEADER_HRSS_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif

// HRSS
//
// HRSS is a structured-lattice-based post-quantum key encapsulation mechanism.
// The best exposition is https://eprint.iacr.org/2017/667.pdf although this
// implementation uses a different KEM construction based on
// https://eprint.iacr.org/2017/1005.pdf.

struct HRSS_private_key {
  uint8_t opaque[1808];
};

struct HRSS_public_key {
  uint8_t opaque[1424];
};

// HRSS_SAMPLE_BYTES is the number of bytes of entropy needed to generate a
// short vector. There are 701 coefficients, but the final one is always set to
// zero when sampling. Otherwise, we need one byte of input per coefficient.
#define HRSS_SAMPLE_BYTES (701 - 1)
// HRSS_GENERATE_KEY_BYTES is the number of bytes of entropy needed to generate
// an HRSS key pair.
#define HRSS_GENERATE_KEY_BYTES (HRSS_SAMPLE_BYTES + HRSS_SAMPLE_BYTES + 32)
// HRSS_ENCAP_BYTES is the number of bytes of entropy needed to encapsulate a
// session key.
#define HRSS_ENCAP_BYTES (HRSS_SAMPLE_BYTES + HRSS_SAMPLE_BYTES)
// HRSS_PUBLIC_KEY_BYTES is the number of bytes in a public key.
#define HRSS_PUBLIC_KEY_BYTES 1138
// HRSS_CIPHERTEXT_BYTES is the number of bytes in a ciphertext.
#define HRSS_CIPHERTEXT_BYTES 1138
// HRSS_KEY_BYTES is the number of bytes in a shared key.
#define HRSS_KEY_BYTES 32
// HRSS_POLY3_BYTES is the number of bytes needed to serialise a mod 3
// polynomial.
#define HRSS_POLY3_BYTES 140
#define HRSS_PRIVATE_KEY_BYTES \
  (HRSS_POLY3_BYTES * 2 + HRSS_PUBLIC_KEY_BYTES + 2 + 32)

// HRSS_generate_key is a deterministic function that outputs a public and
// private key based on the given entropy. It returns one on success or zero
// on malloc failure.
OPENSSL_EXPORT int HRSS_generate_key(
    struct HRSS_public_key *out_pub, struct HRSS_private_key *out_priv,
    const uint8_t input[HRSS_GENERATE_KEY_BYTES]);

// HRSS_encap is a deterministic function the generates and encrypts a random
// session key from the given entropy, writing those values to |out_shared_key|
// and |out_ciphertext|, respectively. It returns one on success or zero on
// malloc failure.
OPENSSL_EXPORT int HRSS_encap(uint8_t out_ciphertext[HRSS_CIPHERTEXT_BYTES],
                              uint8_t out_shared_key[HRSS_KEY_BYTES],
                              const struct HRSS_public_key *in_pub,
                              const uint8_t in[HRSS_ENCAP_BYTES]);

// HRSS_decap decrypts a session key from |ciphertext_len| bytes of
// |ciphertext|. If the ciphertext is valid, the decrypted key is written to
// |out_shared_key|. Otherwise the HMAC of |ciphertext| under a secret key (kept
// in |in_priv|) is written. If the ciphertext is the wrong length then it will
// leak which was done via side-channels. Otherwise it should perform either
// action in constant-time. It returns one on success (whether the ciphertext
// was valid or not) and zero on malloc failure.
OPENSSL_EXPORT int HRSS_decap(uint8_t out_shared_key[HRSS_KEY_BYTES],
                              const struct HRSS_private_key *in_priv,
                              const uint8_t *ciphertext, size_t ciphertext_len);

// HRSS_marshal_public_key serialises |in_pub| to |out|.
OPENSSL_EXPORT void HRSS_marshal_public_key(
    uint8_t out[HRSS_PUBLIC_KEY_BYTES], const struct HRSS_public_key *in_pub);

// HRSS_parse_public_key sets |*out| to the public-key encoded in |in|. It
// returns true on success and zero on error.
OPENSSL_EXPORT int HRSS_parse_public_key(
    struct HRSS_public_key *out, const uint8_t in[HRSS_PUBLIC_KEY_BYTES]);


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_HRSS_H
