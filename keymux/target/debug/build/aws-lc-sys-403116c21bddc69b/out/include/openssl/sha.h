/* Copyright (C) 1995-1998 Eric Young (eay@cryptsoft.com)
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
 * [including the GNU Public Licence.] */

#ifndef OPENSSL_HEADER_SHA_H
#define OPENSSL_HEADER_SHA_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// The SHA family of hash functions (SHA-1 and SHA-2).


// SHA_CBLOCK is the block size of SHA-1.
#define SHA_CBLOCK 64

// SHA_DIGEST_LENGTH is the length of a SHA-1 digest.
#define SHA_DIGEST_LENGTH 20

// SHA1_Init initialises |sha| and returns one.
OPENSSL_EXPORT int SHA1_Init(SHA_CTX *sha);

// SHA1_Update adds |len| bytes from |data| to |sha| and returns one.
OPENSSL_EXPORT int SHA1_Update(SHA_CTX *sha, const void *data, size_t len);

// SHA1_Final adds the final padding to |sha| and writes the resulting digest to
// |out|, which must have at least |SHA_DIGEST_LENGTH| bytes of space. It
// returns one.
OPENSSL_EXPORT int SHA1_Final(uint8_t out[SHA_DIGEST_LENGTH], SHA_CTX *sha);

// SHA1 writes the digest of |len| bytes from |data| to |out| and returns
// |out|. There must be at least |SHA_DIGEST_LENGTH| bytes of space in
// |out|.
OPENSSL_EXPORT uint8_t *SHA1(const uint8_t *data, size_t len,
                             uint8_t out[SHA_DIGEST_LENGTH]);

// SHA1_Transform is a low-level function that performs a single, SHA-1 block
// transformation using the state from |sha| and |SHA_CBLOCK| bytes from
// |block|.
OPENSSL_EXPORT void SHA1_Transform(SHA_CTX *sha,
                                   const uint8_t block[SHA_CBLOCK]);

struct sha_state_st {
  uint32_t h[5];
  uint32_t Nl, Nh;
  uint8_t data[SHA_CBLOCK];
  unsigned num;
};


// SHA-224.

// SHA224_CBLOCK is the block size of SHA-224.
#define SHA224_CBLOCK 64

// SHA224_DIGEST_LENGTH is the length of a SHA-224 digest.
#define SHA224_DIGEST_LENGTH 28

// SHA224_Init initialises |sha| and returns 1.
OPENSSL_EXPORT int SHA224_Init(SHA256_CTX *sha);

// SHA224_Update adds |len| bytes from |data| to |sha| and returns 1.
OPENSSL_EXPORT int SHA224_Update(SHA256_CTX *sha, const void *data, size_t len);

// SHA224_Final adds the final padding to |sha| and writes the resulting digest
// to |out|, which must have at least |SHA224_DIGEST_LENGTH| bytes of space. It
// returns one on success and zero on programmer error.
OPENSSL_EXPORT int SHA224_Final(uint8_t out[SHA224_DIGEST_LENGTH],
                                SHA256_CTX *sha);

// SHA224 writes the digest of |len| bytes from |data| to |out| and returns
// |out|. There must be at least |SHA224_DIGEST_LENGTH| bytes of space in
// |out|.
OPENSSL_EXPORT uint8_t *SHA224(const uint8_t *data, size_t len,
                               uint8_t out[SHA224_DIGEST_LENGTH]);


// SHA-256.

// SHA256_CBLOCK is the block size of SHA-256.
#define SHA256_CBLOCK 64

// SHA256_DIGEST_LENGTH is the length of a SHA-256 digest.
#define SHA256_DIGEST_LENGTH 32

// SHA256_Init initialises |sha| and returns 1.
OPENSSL_EXPORT int SHA256_Init(SHA256_CTX *sha);

// SHA256_Update adds |len| bytes from |data| to |sha| and returns 1.
OPENSSL_EXPORT int SHA256_Update(SHA256_CTX *sha, const void *data, size_t len);

// SHA256_Final adds the final padding to |sha| and writes the resulting digest
// to |out|, which must have at least |SHA256_DIGEST_LENGTH| bytes of space. It
// returns one on success and zero on programmer error.
OPENSSL_EXPORT int SHA256_Final(uint8_t out[SHA256_DIGEST_LENGTH],
                                SHA256_CTX *sha);

// SHA256 writes the digest of |len| bytes from |data| to |out| and returns
// |out|. There must be at least |SHA256_DIGEST_LENGTH| bytes of space in
// |out|.
OPENSSL_EXPORT uint8_t *SHA256(const uint8_t *data, size_t len,
                               uint8_t out[SHA256_DIGEST_LENGTH]);

// SHA256_Transform is a low-level function that performs a single, SHA-256
// block transformation using the state from |sha| and |SHA256_CBLOCK| bytes
// from |block|.
OPENSSL_EXPORT void SHA256_Transform(SHA256_CTX *sha,
                                     const uint8_t block[SHA256_CBLOCK]);

// SHA256_TransformBlocks is a low-level function that takes |num_blocks| *
// |SHA256_CBLOCK| bytes of data and performs SHA-256 transforms on it to update
// |state|. You should not use this function unless you are implementing a
// derivative of SHA-256.
OPENSSL_EXPORT void SHA256_TransformBlocks(uint32_t state[8],
                                           const uint8_t *data,
                                           size_t num_blocks);

struct sha256_state_st {
  uint32_t h[8];
  uint32_t Nl, Nh;
  uint8_t data[SHA256_CBLOCK];
  unsigned num, md_len;
};


// SHA-384.

// SHA384_CBLOCK is the block size of SHA-384.
#define SHA384_CBLOCK 128

// SHA384_DIGEST_LENGTH is the length of a SHA-384 digest.
#define SHA384_DIGEST_LENGTH 48

// SHA384_Init initialises |sha| and returns 1.
OPENSSL_EXPORT int SHA384_Init(SHA512_CTX *sha);

// SHA384_Update adds |len| bytes from |data| to |sha| and returns 1.
OPENSSL_EXPORT int SHA384_Update(SHA512_CTX *sha, const void *data, size_t len);

// SHA384_Final adds the final padding to |sha| and writes the resulting digest
// to |out|, which must have at least |SHA384_DIGEST_LENGTH| bytes of space. It
// returns one on success and zero on programmer error.
OPENSSL_EXPORT int SHA384_Final(uint8_t out[SHA384_DIGEST_LENGTH],
                                SHA512_CTX *sha);

// SHA384 writes the digest of |len| bytes from |data| to |out| and returns
// |out|. There must be at least |SHA384_DIGEST_LENGTH| bytes of space in
// |out|.
OPENSSL_EXPORT uint8_t *SHA384(const uint8_t *data, size_t len,
                               uint8_t out[SHA384_DIGEST_LENGTH]);


// SHA-512.

// SHA512_CBLOCK is the block size of SHA-512.
#define SHA512_CBLOCK 128

// SHA512_DIGEST_LENGTH is the length of a SHA-512 digest.
#define SHA512_DIGEST_LENGTH 64

// SHA512_Init initialises |sha| and returns 1.
OPENSSL_EXPORT int SHA512_Init(SHA512_CTX *sha);

// SHA512_Update adds |len| bytes from |data| to |sha| and returns 1.
OPENSSL_EXPORT int SHA512_Update(SHA512_CTX *sha, const void *data, size_t len);

// SHA512_Final adds the final padding to |sha| and writes the resulting digest
// to |out|, which must have at least |SHA512_DIGEST_LENGTH| bytes of space. It
// returns one on success and zero on programmer error.
OPENSSL_EXPORT int SHA512_Final(uint8_t out[SHA512_DIGEST_LENGTH],
                                SHA512_CTX *sha);

// SHA512 writes the digest of |len| bytes from |data| to |out| and returns
// |out|. There must be at least |SHA512_DIGEST_LENGTH| bytes of space in
// |out|.
OPENSSL_EXPORT uint8_t *SHA512(const uint8_t *data, size_t len,
                               uint8_t out[SHA512_DIGEST_LENGTH]);

// SHA512_Transform is a low-level function that performs a single, SHA-512
// block transformation using the state from |sha| and |SHA512_CBLOCK| bytes
// from |block|.
OPENSSL_EXPORT void SHA512_Transform(SHA512_CTX *sha,
                                     const uint8_t block[SHA512_CBLOCK]);

struct sha512_state_st {
  uint64_t h[8];
  uint64_t Nl, Nh;
  uint8_t p[128];
  unsigned num, md_len;
};


// SHA-512-224 and SHA-512-256
//
// See https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.180-4.pdf section 5.3.6

#define SHA512_224_DIGEST_LENGTH 28

// SHA512_224_Init initialises |sha| and returns 1.
OPENSSL_EXPORT int SHA512_224_Init(SHA512_CTX *sha);

// SHA512_224_Update adds |len| bytes from |data| to |sha| and returns 1.
OPENSSL_EXPORT int SHA512_224_Update(SHA512_CTX *sha, const void *data,
                                     size_t len);

// SHA512_224_Final adds the final padding to |sha| and writes the resulting
// digest to |out|, which must have at least |SHA512_224_DIGEST_LENGTH| bytes of
// space. It returns one on success and zero on programmer error.
OPENSSL_EXPORT int SHA512_224_Final(uint8_t out[SHA512_224_DIGEST_LENGTH],
                                    SHA512_CTX *sha);

// SHA512_224 writes the digest of |len| bytes from |data| to |out| and returns
// |out|. There must be at least |SHA512_224_DIGEST_LENGTH| bytes of space in
// |out|.
OPENSSL_EXPORT uint8_t *SHA512_224(const uint8_t *data, size_t len,
                                   uint8_t out[SHA512_224_DIGEST_LENGTH]);

#define SHA512_256_DIGEST_LENGTH 32

// SHA512_256_Init initialises |sha| and returns 1.
OPENSSL_EXPORT int SHA512_256_Init(SHA512_CTX *sha);

// SHA512_256_Update adds |len| bytes from |data| to |sha| and returns 1.
OPENSSL_EXPORT int SHA512_256_Update(SHA512_CTX *sha, const void *data,
                                     size_t len);

// SHA512_256_Final adds the final padding to |sha| and writes the resulting
// digest to |out|, which must have at least |SHA512_256_DIGEST_LENGTH| bytes of
// space. It returns one on success and zero on programmer error.
OPENSSL_EXPORT int SHA512_256_Final(uint8_t out[SHA512_256_DIGEST_LENGTH],
                                    SHA512_CTX *sha);

// SHA512_256 writes the digest of |len| bytes from |data| to |out| and returns
// |out|. There must be at least |SHA512_256_DIGEST_LENGTH| bytes of space in
// |out|.
OPENSSL_EXPORT uint8_t *SHA512_256(const uint8_t *data, size_t len,
                                   uint8_t out[SHA512_256_DIGEST_LENGTH]);


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_SHA_H
