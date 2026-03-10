/* Copyright (c) 2015, Google Inc.
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

#ifndef OPENSSL_HEADER_CMAC_H
#define OPENSSL_HEADER_CMAC_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// CMAC.
//
// CMAC is a MAC based on AES-CBC and defined in
// https://tools.ietf.org/html/rfc4493#section-2.3.


// One-shot functions.

// AES_CMAC calculates the 16-byte, CMAC authenticator of |in_len| bytes of
// |in| and writes it to |out|. The |key_len| may be 16 or 32 bytes to select
// between AES-128 and AES-256. It returns one on success or zero on error.
OPENSSL_EXPORT int AES_CMAC(uint8_t out[16], const uint8_t *key, size_t key_len,
                            const uint8_t *in, size_t in_len);


// Incremental interface.

// CMAC_CTX_new allocates a fresh |CMAC_CTX| and returns it, or NULL on
// error.
OPENSSL_EXPORT CMAC_CTX *CMAC_CTX_new(void);

// CMAC_CTX_free frees a |CMAC_CTX|.
OPENSSL_EXPORT void CMAC_CTX_free(CMAC_CTX *ctx);

// CMAC_CTX_copy sets |out| to be a duplicate of the current state |in|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int CMAC_CTX_copy(CMAC_CTX *out, const CMAC_CTX *in);

// CMAC_Init configures |ctx| to use the given |key| and |cipher|. The CMAC RFC
// only specifies the use of AES-128 thus |key_len| should be 16 and |cipher|
// should be |EVP_aes_128_cbc()|. However, this implementation also supports
// AES-256 by setting |key_len| to 32 and |cipher| to |EVP_aes_256_cbc()|. The
// |engine| argument is ignored.
//
// It returns one on success or zero on error.
OPENSSL_EXPORT int CMAC_Init(CMAC_CTX *ctx, const void *key, size_t key_len,
                             const EVP_CIPHER *cipher, ENGINE *engine);


// CMAC_Reset resets |ctx| so that a fresh message can be authenticated.
OPENSSL_EXPORT int CMAC_Reset(CMAC_CTX *ctx);

// CMAC_Update processes |in_len| bytes of message from |in|. It returns one on
// success or zero on error.
OPENSSL_EXPORT int CMAC_Update(CMAC_CTX *ctx, const uint8_t *in, size_t in_len);

// CMAC_Final sets |*out_len| to 16 and, if |out| is not NULL, writes 16 bytes
// of authenticator to it. It returns one on success or zero on error.
OPENSSL_EXPORT int CMAC_Final(CMAC_CTX *ctx, uint8_t *out, size_t *out_len);


// Deprecated functions.

// CMAC_CTX_get0_cipher_ctx returns a pointer to the |EVP_CIPHER_CTX| from |ctx|.
OPENSSL_EXPORT EVP_CIPHER_CTX *CMAC_CTX_get0_cipher_ctx(CMAC_CTX *ctx);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(CMAC_CTX, CMAC_CTX_free)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#endif  // OPENSSL_HEADER_CMAC_H
