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

#ifndef OPENSSL_HEADER_HMAC_H
#define OPENSSL_HEADER_HMAC_H

#include <openssl/base.h>

#include <openssl/digest.h>
#include <openssl/sha.h>
#include <openssl/md5.h>

#if defined(__cplusplus)
extern "C" {
#endif


// HMAC contains functions for constructing PRFs from Merkle–Damgård hash
// functions using HMAC.


// One-shot operation.

// HMAC calculates the HMAC of |data_len| bytes of |data|, using the given key
// and hash function, and writes the result to |out|. On entry, |out| must
// contain at least |EVP_MD_size| bytes of space. The actual length of the
// result is written to |*out_len|. An output size of |EVP_MAX_MD_SIZE| will
// always be large enough. It returns |out| or NULL on error.
OPENSSL_EXPORT uint8_t *HMAC(const EVP_MD *evp_md, const void *key,
                             size_t key_len, const uint8_t *data,
                             size_t data_len, uint8_t *out,
                             unsigned int *out_len);


// Incremental operation.

// HMAC_CTX_init initialises |ctx| for use in an HMAC operation. It's assumed
// that HMAC_CTX objects will be allocated on the stack thus no allocation
// function is provided.
OPENSSL_EXPORT void HMAC_CTX_init(HMAC_CTX *ctx);

// HMAC_CTX_new allocates and initialises a new |HMAC_CTX| and returns it, or
// NULL on allocation failure. The caller must use |HMAC_CTX_free| to release
// the resulting object.
OPENSSL_EXPORT HMAC_CTX *HMAC_CTX_new(void);

// HMAC_CTX_cleanup zeroises |ctx| since it's allocated on the stack.
// This brings the context to its initial state.
OPENSSL_EXPORT void HMAC_CTX_cleanup(HMAC_CTX *ctx);

// HMAC_CTX_cleanse calls |HMAC_CTX_cleanup|.
OPENSSL_EXPORT void HMAC_CTX_cleanse(HMAC_CTX *ctx);

// HMAC_CTX_free calls |HMAC_CTX_cleanup| and then frees |ctx| itself.
OPENSSL_EXPORT void HMAC_CTX_free(HMAC_CTX *ctx);

// HMAC_Init_ex sets up an initialised |HMAC_CTX| to use |md| as the hash
// function and |key| as the key. This function resets |HMAC_CTX| to a
// fresh state, even if |HMAC_Update| or |HMAC_Final| have been called
// previously. For a non-initial call, |md| may be NULL, in which case the
// previous hash function will be used. If the hash function has not changed and
// |key| is NULL, |ctx| reuses the previous key and resets to a clean state
// ready for new data. It returns one on success or zero on allocation failure.
//
// WARNING: NULL and empty keys are ambiguous on non-initial calls. Passing NULL
// |key| but repeating the previous |md| reuses the previous key rather than the
// empty key.
OPENSSL_EXPORT int HMAC_Init_ex(HMAC_CTX *ctx, const void *key, size_t key_len,
                                const EVP_MD *md, ENGINE *impl);

// HMAC_Update hashes |data_len| bytes from |data| into the current HMAC
// operation in |ctx|. It returns one.
OPENSSL_EXPORT int HMAC_Update(HMAC_CTX *ctx, const uint8_t *data,
                               size_t data_len);

// HMAC_Final completes the HMAC operation in |ctx| and writes the result to
// |out| and then sets |*out_len| to the length of the result. On entry, |out|
// must contain at least |HMAC_size| bytes of space. An output size of
// |EVP_MAX_MD_SIZE| will always be large enough. It returns one on success or
// zero on allocation failure.
OPENSSL_EXPORT int HMAC_Final(HMAC_CTX *ctx, uint8_t *out,
                              unsigned int *out_len);


// Utility functions.

// HMAC_size returns the size, in bytes, of the HMAC that will be produced by
// |ctx|. On entry, |ctx| must have been setup with |HMAC_Init_ex|.
OPENSSL_EXPORT size_t HMAC_size(const HMAC_CTX *ctx);

// HMAC_CTX_get_md returns |ctx|'s hash function.
OPENSSL_EXPORT const EVP_MD *HMAC_CTX_get_md(const HMAC_CTX *ctx);

// HMAC_CTX_copy_ex sets |dest| equal to |src|. On entry, |dest| must have been
// initialised by calling |HMAC_CTX_init|. It returns one on success and zero
// on error.
OPENSSL_EXPORT int HMAC_CTX_copy_ex(HMAC_CTX *dest, const HMAC_CTX *src);

// HMAC_CTX_reset calls |HMAC_CTX_cleanup| followed by |HMAC_CTX_init|.
OPENSSL_EXPORT void HMAC_CTX_reset(HMAC_CTX *ctx);


// Precomputed key functions

// HMAC_MD5_PRECOMPUTED_KEY_SIZE is the precomputed key size for MD5, in bytes
#define HMAC_MD5_PRECOMPUTED_KEY_SIZE 32
// HMAC_SHA1_PRECOMPUTED_KEY_SIZE is the precomputed key size for SHA1, in bytes
#define HMAC_SHA1_PRECOMPUTED_KEY_SIZE 40
// HMAC_SHA224_PRECOMPUTED_KEY_SIZE is the precomputed key size for SHA224, in bytes
#define HMAC_SHA224_PRECOMPUTED_KEY_SIZE 64
// HMAC_SHA256_PRECOMPUTED_KEY_SIZE is the precomputed key size for SHA256, in bytes
#define HMAC_SHA256_PRECOMPUTED_KEY_SIZE 64
// HMAC_SHA384_PRECOMPUTED_KEY_SIZE is the precomputed key size for SHA384, in bytes
#define HMAC_SHA384_PRECOMPUTED_KEY_SIZE 128
// HMAC_SHA512_PRECOMPUTED_KEY_SIZE is the precomputed key size for SHA512, in bytes
#define HMAC_SHA512_PRECOMPUTED_KEY_SIZE 128
// HMAC_SHA512_224_PRECOMPUTED_KEY_SIZE is the precomputed key size for SHA512_224, in bytes
#define HMAC_SHA512_224_PRECOMPUTED_KEY_SIZE 128
// HMAC_SHA512_256_PRECOMPUTED_KEY_SIZE is the precomputed key size for SHA512_256, in bytes
#define HMAC_SHA512_256_PRECOMPUTED_KEY_SIZE 128

// HMAC_MAX_PRECOMPUTED_KEY_SIZE is the largest precomputed key size, in bytes.
#define HMAC_MAX_PRECOMPUTED_KEY_SIZE (2 * (EVP_MAX_MD_CHAINING_LENGTH))

// HMAC_set_precomputed_key_export sets the context |ctx| to allow export of the
// precomputed key using HMAC_get_precomputed_key. On entry, HMAC_CTX must have
// been initialized via HMAC_Init_*, and neither HMAC_Update nor HMAC_Final
// must have been called after the last HMAC_Init_ex. It returns one on success
// and zero on error.
// After a successful call to HMAC_set_precomputed_key_export, HMAC_Update and
// HMAC_Final will fail.
//
// Note: The main reason for this function is to satisfy FIPS assertion AS09.16,
// since HMAC_get_precomputed_key returns key material (i.e., a CSP in NIST
// terminology).
OPENSSL_EXPORT int HMAC_set_precomputed_key_export(HMAC_CTX *ctx);

// HMAC_get_precomputed_key exports the precomputed key. If |out| is NULL,
// |out_len| is set to the size of the precomputed key. After such a call,
// |HMAC_get_precomputed_key| can directly be called again with a non-null
// |out|. But |HMAC_Update| and |HMAC_Final| will still fail.
//
// If |out| is not NULL, |*out_len| must contain the number of bytes of space
// available at |out|. If sufficient, the precomputed key will be written in
// |out| and |out_len| will be updated with the true length (which is
// |HMAC_xxx_PRECOMPUTED_KEY_SIZE| for hash function xxx). An output size of
// |HMAC_MAX_PRECOMPUTED_KEY_SIZE| will always be large enough. After a
// successful call to |HMAC_get_precomputed_key| with a non-NULL |out|, the
// context can be directly used for computing an HMAC using |HMAC_Update| and
// |HMAC_Final|.
//
// The function returns one on success and zero on error.
//
// The precomputed key is the concatenation:
//   precomputed_key = key_ipad || key_opad
// where:
//   key_ipad = Hash_Compression_Function(key' xor ipad)
//   key_opad = Hash_Compression_Function(key' xor opad)
//   key' = padding of key with 0 on the right to be of the block length
//                if length of key is at most the block length
//          or Hash(key)
//                otherwise
//
// Knowledge of precomputed_key is sufficient to compute HMAC. Use of the
// precomputed key instead of the key reduces by 2 the number of hash
// compression function calls (or more if key is larger than the block length)
OPENSSL_EXPORT int HMAC_get_precomputed_key(HMAC_CTX *ctx, uint8_t *out,
                                           size_t *out_len);

// HMAC_Init_from_precomputed_key sets up an initialised |HMAC_CTX| to use
// |md| as the hash function and |precomputed_key| as the precomputed key
// (see |HMAC_get_precomputed_key|).
// For a non-initial call, |md| may be NULL, in which case the previous hash
// function is used. If the hash function has not changed and |precomputed_key|
// is NULL, the previous key is used. This non-initial call is interchangeable
// with calling |HMAC_Init_ex| with the same parameters. It returns one on
// success or zero on failure.
//
// Note: Contrary to input keys to |HMAC_Init_ex|, which can be the empty key,
//   an input precomputed key cannot be empty in an initial call to
//   |HMAC_Init_from_precomputed_key|. Otherwise, the call fails and returns zero.
OPENSSL_EXPORT int HMAC_Init_from_precomputed_key(HMAC_CTX *ctx,
                                                 const uint8_t *precomputed_key,
                                                 size_t precompute_key_len,
                                                 const EVP_MD *md);


// Deprecated functions.

OPENSSL_EXPORT int HMAC_Init(HMAC_CTX *ctx, const void *key, int key_len,
                             const EVP_MD *md);

// HMAC_CTX_copy calls |HMAC_CTX_init| on |dest| and then sets it equal to
// |src|. On entry, |dest| must /not/ be initialised for an operation with
// |HMAC_Init_ex|. It returns one on success and zero on error.
OPENSSL_EXPORT int HMAC_CTX_copy(HMAC_CTX *dest, const HMAC_CTX *src);


// Private functions
typedef struct hmac_methods_st HmacMethods;

// We use a union to ensure that enough space is allocated and never actually
// bother with the named members. We do not externalize SHA3 ctx definition,
// so hard-code ctx size below and use a compile-time assertion where that ctx
// is defined to ensure it does not exceed size bounded by |md_ctx_union|. This
// is OK because union members are never referenced, they're only used for sizing.
union md_ctx_union {
  MD5_CTX md5;
  SHA_CTX sha1;
  SHA256_CTX sha256;
  SHA512_CTX sha512;
  uint8_t sha3[400];
};

struct hmac_ctx_st {
  const EVP_MD *md;
  const HmacMethods *methods;
  union md_ctx_union md_ctx;
  union md_ctx_union i_ctx;
  union md_ctx_union o_ctx;
  int8_t state;
} /* HMAC_CTX */;


#if defined(__cplusplus)
}  // extern C

#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(HMAC_CTX, HMAC_CTX_free)

using ScopedHMAC_CTX =
    internal::StackAllocated<HMAC_CTX, void, HMAC_CTX_init, HMAC_CTX_cleanup>;

BSSL_NAMESPACE_END

}  // extern C++
#endif

#endif


// Errors

#define HMAC_R_MISSING_PARAMETERS 100
#define HMAC_R_BUFFER_TOO_SMALL 102
#define HMAC_R_SET_PRECOMPUTED_KEY_EXPORT_NOT_CALLED 103
#define HMAC_R_NOT_CALLED_JUST_AFTER_INIT 104
#define HMAC_R_PRECOMPUTED_KEY_NOT_SUPPORTED_FOR_DIGEST 105

#endif  // OPENSSL_HEADER_HMAC_H
