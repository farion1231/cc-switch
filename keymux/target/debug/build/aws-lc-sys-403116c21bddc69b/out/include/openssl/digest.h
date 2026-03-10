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

#ifndef OPENSSL_HEADER_DIGEST_H
#define OPENSSL_HEADER_DIGEST_H

#include <stdbool.h>

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif

// Digest functions.
//
// An EVP_MD abstracts the details of a specific hash function allowing code to
// deal with the concept of a "hash function" without needing to know exactly
// which hash function it is.


// Hash algorithms.
//
// The following functions return |EVP_MD| objects that implement the named hash
// function.

OPENSSL_EXPORT const EVP_MD *EVP_md4(void);
OPENSSL_EXPORT const EVP_MD *EVP_md5(void);
OPENSSL_EXPORT const EVP_MD *EVP_ripemd160(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha1(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha224(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha256(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha384(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha512(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha512_224(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha512_256(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha3_224(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha3_256(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha3_384(void);
OPENSSL_EXPORT const EVP_MD *EVP_sha3_512(void);
OPENSSL_EXPORT const EVP_MD *EVP_shake128(void);
OPENSSL_EXPORT const EVP_MD *EVP_shake256(void);
OPENSSL_EXPORT const EVP_MD *EVP_blake2b256(void);

// EVP_md5_sha1 is a TLS-specific |EVP_MD| which computes the concatenation of
// MD5 and SHA-1, as used in TLS 1.1 and below.
OPENSSL_EXPORT const EVP_MD *EVP_md5_sha1(void);

// EVP_get_digestbynid returns an |EVP_MD| for the given NID, or NULL if no
// such digest is known.
OPENSSL_EXPORT const EVP_MD *EVP_get_digestbynid(int nid);

// EVP_get_digestbyobj returns an |EVP_MD| for the given |ASN1_OBJECT|, or NULL
// if no such digest is known.
OPENSSL_EXPORT const EVP_MD *EVP_get_digestbyobj(const ASN1_OBJECT *obj);


// Digest contexts.
//
// An EVP_MD_CTX represents the state of a specific digest operation in
// progress.

// EVP_MD_CTX_init initialises an, already allocated, |EVP_MD_CTX|. This is the
// same as setting the structure to zero.
OPENSSL_EXPORT void EVP_MD_CTX_init(EVP_MD_CTX *ctx);

// EVP_MD_CTX_new allocates and initialises a fresh |EVP_MD_CTX| and returns
// it, or NULL on allocation failure. The caller must use |EVP_MD_CTX_free| to
// release the resulting object.
OPENSSL_EXPORT EVP_MD_CTX *EVP_MD_CTX_new(void);

// EVP_MD_CTX_cleanup frees any resources owned by |ctx| and resets it to a
// freshly initialised state. It does not free |ctx| itself. It returns one.
OPENSSL_EXPORT int EVP_MD_CTX_cleanup(EVP_MD_CTX *ctx);

// EVP_MD_CTX_cleanse zeros the digest state in |ctx| and then performs the
// actions of |EVP_MD_CTX_cleanup|. Note that some |EVP_MD_CTX| objects contain
// more than just a digest (e.g. those resulting from |EVP_DigestSignInit|) but
// this function does not zero out more than just the digest state even in that
// case.
OPENSSL_EXPORT void EVP_MD_CTX_cleanse(EVP_MD_CTX *ctx);

// EVP_MD_CTX_free calls |EVP_MD_CTX_cleanup| and then frees |ctx| itself.
OPENSSL_EXPORT void EVP_MD_CTX_free(EVP_MD_CTX *ctx);

// EVP_MD_CTX_copy_ex sets |out|, which must already be initialised, to be a
// copy of |in|. It returns one on success and zero on allocation failure.
OPENSSL_EXPORT int EVP_MD_CTX_copy_ex(EVP_MD_CTX *out, const EVP_MD_CTX *in);

// EVP_MD_CTX_move sets |out|, which must already be initialised, to the hash
// state in |in|. |in| is mutated and left in an empty state.
OPENSSL_EXPORT void EVP_MD_CTX_move(EVP_MD_CTX *out, EVP_MD_CTX *in);

// EVP_MD_CTX_reset calls |EVP_MD_CTX_cleanup| followed by |EVP_MD_CTX_init|. It
// returns one.
OPENSSL_EXPORT int EVP_MD_CTX_reset(EVP_MD_CTX *ctx);


// Digest operations.

// EVP_DigestInit_ex configures |ctx|, which must already have been
// initialised, for a fresh hashing operation using |type|. It returns one on
// success and zero on allocation failure.
OPENSSL_EXPORT int EVP_DigestInit_ex(EVP_MD_CTX *ctx, const EVP_MD *type,
                                     ENGINE *engine);

// EVP_DigestInit acts like |EVP_DigestInit_ex| except that |ctx| is
// initialised before use.
OPENSSL_EXPORT int EVP_DigestInit(EVP_MD_CTX *ctx, const EVP_MD *type);

// EVP_DigestUpdate hashes |len| bytes from |data| into the hashing operation
// in |ctx|. It returns one.
OPENSSL_EXPORT int EVP_DigestUpdate(EVP_MD_CTX *ctx, const void *data,
                                    size_t len);

// EVP_MAX_MD_SIZE is the largest digest size supported, in bytes.
// Functions that output a digest generally require the buffer have
// at least this much space.
#define EVP_MAX_MD_SIZE 64  // SHA-512 is the longest so far.

// EVP_MAX_MD_CHAINING_LENGTH is the largest chaining length supported, in
// bytes. This constant is only for Merkle-Damgard-based hashed functions
// like SHA-1, SHA-2, and MD5. The chaining length is defined as the output
// length of the hash in bytes, before any truncation (e.g., 32 for SHA-224 and
// SHA-256, 64 for SHA-384 and SHA-512).
// This constant is only used internally by HMAC.
#define EVP_MAX_MD_CHAINING_LENGTH 64  // SHA-512 has the longest chaining length so far

// EVP_MAX_MD_BLOCK_SIZE is the largest digest block size supported, in
// bytes.
#define EVP_MAX_MD_BLOCK_SIZE 144      // SHA3-224 has the largest block size so far

// EVP_DigestFinal_ex finishes the digest in |ctx| and writes the output to
// |md_out|. |EVP_MD_CTX_size| bytes are written, which is at most
// |EVP_MAX_MD_SIZE|. If |out_size| is not NULL then |*out_size| is set to the
// number of bytes written. It returns one. After this call, the hash cannot be
// updated or finished again until |EVP_DigestInit_ex| is called to start
// another hashing operation.
OPENSSL_EXPORT int EVP_DigestFinal_ex(EVP_MD_CTX *ctx, uint8_t *md_out,
                                      unsigned int *out_size);

// EVP_DigestFinal acts like |EVP_DigestFinal_ex| except that
// |EVP_MD_CTX_cleanup| is called on |ctx| before returning.
OPENSSL_EXPORT int EVP_DigestFinal(EVP_MD_CTX *ctx, uint8_t *md_out,
                                   unsigned int *out_size);

// EVP_Digest performs a complete hashing operation in one call. It hashes |len|
// bytes from |data| and writes the digest to |md_out|. |EVP_MD_CTX_size| bytes
// are written, which is at most |EVP_MAX_MD_SIZE|. If |out_size| is not NULL
// then |*out_size| is set to the number of bytes written. It returns one on
// success and zero otherwise. If |type| is an XOF, |out_size| must be set to
// the desired output length.
OPENSSL_EXPORT int EVP_Digest(const void *data, size_t len, uint8_t *md_out,
                              unsigned int *out_size, const EVP_MD *type,
                              ENGINE *impl);


// Digest function accessors.
//
// These functions allow code to learn details about an abstract hash
// function.

// EVP_MD_type returns a NID identifying |md|. (For example, |NID_sha256|.)
OPENSSL_EXPORT int EVP_MD_type(const EVP_MD *md);

// EVP_MD_flags returns the flags for |md|, which is a set of |EVP_MD_FLAG_*|
// values, ORed together.
OPENSSL_EXPORT uint32_t EVP_MD_flags(const EVP_MD *md);

// EVP_MD_size returns the digest size of |md|, in bytes.
OPENSSL_EXPORT size_t EVP_MD_size(const EVP_MD *md);

// EVP_MD_block_size returns the native block-size of |md|, in bytes.
OPENSSL_EXPORT size_t EVP_MD_block_size(const EVP_MD *md);

// EVP_MD_FLAG_DIGALGID_ABSENT indicates that the parameter type in an X.509
// DigestAlgorithmIdentifier representing this digest function should be
// undefined rather than NULL.
#define EVP_MD_FLAG_DIGALGID_ABSENT 2

// EVP_MD_FLAG_XOF indicates that the digest is an extensible-output function
// (XOF).
#define EVP_MD_FLAG_XOF 4


// Digest operation accessors.

// EVP_MD_CTX_md returns the underlying digest function, or NULL if one has not
// been set.
OPENSSL_EXPORT const EVP_MD *EVP_MD_CTX_md(const EVP_MD_CTX *ctx);

// EVP_MD_CTX_size returns the digest size of |ctx|, in bytes. It
// will crash if a digest hasn't been set on |ctx|.
OPENSSL_EXPORT size_t EVP_MD_CTX_size(const EVP_MD_CTX *ctx);

// EVP_MD_CTX_block_size returns the block size of the digest function used by
// |ctx|, in bytes. It will crash if a digest hasn't been set on |ctx|.
OPENSSL_EXPORT size_t EVP_MD_CTX_block_size(const EVP_MD_CTX *ctx);

// EVP_MD_CTX_type returns a NID describing the digest function used by |ctx|.
// (For example, |NID_sha256|.) It will crash if a digest hasn't been set on
// |ctx|.
OPENSSL_EXPORT int EVP_MD_CTX_type(const EVP_MD_CTX *ctx);


// ASN.1 functions.
//
// These functions allow code to parse and serialize AlgorithmIdentifiers for
// hash functions.

// EVP_parse_digest_algorithm parses an AlgorithmIdentifier structure containing
// a hash function OID (for example, 2.16.840.1.101.3.4.2.1 is SHA-256) and
// advances |cbs|. The parameters field may either be omitted or a NULL. It
// returns the digest function or NULL on error.
OPENSSL_EXPORT const EVP_MD *EVP_parse_digest_algorithm(CBS *cbs);

// EVP_marshal_digest_algorithm marshals |md| as an AlgorithmIdentifier
// structure and appends the result to |cbb|. It returns one on success and zero
// on error.
OPENSSL_EXPORT int EVP_marshal_digest_algorithm(CBB *cbb, const EVP_MD *md);


// Deprecated functions.

// EVP_MD_CTX_copy sets |out|, which must /not/ be initialised, to be a copy of
// |in|. It returns one on success and zero on error.
OPENSSL_EXPORT int EVP_MD_CTX_copy(EVP_MD_CTX *out, const EVP_MD_CTX *in);

// EVP_get_digestbyname returns an |EVP_MD| given a human readable name in
// |name|, or NULL if the name is unknown.
OPENSSL_EXPORT const EVP_MD *EVP_get_digestbyname(const char *);

// EVP_MD_CTX_create calls |EVP_MD_CTX_new|.
OPENSSL_EXPORT EVP_MD_CTX *EVP_MD_CTX_create(void);

// EVP_MD_CTX_destroy calls |EVP_MD_CTX_free|.
OPENSSL_EXPORT void EVP_MD_CTX_destroy(EVP_MD_CTX *ctx);

// EVP_DigestFinalXOF behaves like |EVP_DigestFinal| for XOF digests, writing
// |len| bytes of extended output to |out|.
OPENSSL_EXPORT int EVP_DigestFinalXOF(EVP_MD_CTX *ctx, uint8_t *out,
                                      size_t len);

// EVP_DigestSqueeze provides byte-wise streaming XOF output generation for
// XOF digests, writing |len| bytes of extended output to |out|. It can be
// called multiple times with arbitrary length |len| output requests.
// It returns one on success and zero on error.
OPENSSL_EXPORT int EVP_DigestSqueeze(EVP_MD_CTX *ctx, uint8_t *out,
                                      size_t len);

// EVP_MD_meth_get_flags calls |EVP_MD_flags|.
OPENSSL_EXPORT uint32_t EVP_MD_meth_get_flags(const EVP_MD *md);

// EVP_MD_nid calls |EVP_MD_type|.
OPENSSL_EXPORT int EVP_MD_nid(const EVP_MD *md);

// EVP_MD_CTX_set_pkey_ctx sets |ctx|'s |EVP_PKEY_CTX| reference to |pctx|.
// The |EVP_PKEY_CTX| object |pctx| needs to have been initialised before
// associating it with |ctx|. The hash functions associated to |ctx| and |pctx|
// must be equal. Once |EVP_MD_CTX_set_pkey_ctx| is called, the caller is
// responsible for freeing |pctx|. Calling |EVP_MD_CTX_cleanup| will not free
// |pctx|.
//
// A NULL |pctx| pointer is also allowed to set the |EVP_PKEY_CTX| reference
// inside |ctx| to NULL. However, even when doing so, the caller is still
// responsible for freeing the |pctx| pointer that had originally been
// associated.
//
// |EVP_MD_CTX_set_pkey_ctx| will overwrite any |EVP_PKEY_CTX| object associated
// to |ctx|. If it was not associated through a previous
// |EVP_MD_CTX_set_pkey_ctx| call, it will be freed first.
OPENSSL_EXPORT void EVP_MD_CTX_set_pkey_ctx(EVP_MD_CTX *ctx,
                                            EVP_PKEY_CTX *pctx);

// EVP_MD_CTX_get_pkey_ctx returns the pointer of |ctx|'s |EVP_PKEY_CTX|.
OPENSSL_EXPORT EVP_PKEY_CTX *EVP_MD_CTX_get_pkey_ctx(const EVP_MD_CTX *ctx);

// EVP_MD_CTX_pkey_ctx is a legacy alias of |EVP_MD_CTX_get_pkey_ctx|.
OPENSSL_EXPORT EVP_PKEY_CTX *EVP_MD_CTX_pkey_ctx(const EVP_MD_CTX *ctx);

struct evp_md_pctx_ops;

// env_md_ctx_st is typoed ("evp" -> "env"), but the typo comes from OpenSSL
// and some consumers forward-declare these structures so we're leaving it
// alone.
struct env_md_ctx_st {
  // digest is the underlying digest function, or NULL if not set.
  const EVP_MD *digest;
  // md_data points to a block of memory that contains the hash-specific
  // context.
  void *md_data;

  // update is usually copied from |digest->update|. This is only different
  // when consumed through |EVP_PKEY_HMAC|.
  // TODO: Look into untangling this, so that |EVP_PKEY_HMAC| can directly call
  // |digest->update|. |digest->update| operates against |md_data| above, but
  // |HMAC_CTX| maintains its own data state in |HMAC_CTX->md_ctx|.
  // |HMAC_Update| also has an additional state transition to handle.
  int (*update)(EVP_MD_CTX *ctx, const void *data, size_t count);

  // pctx is an opaque (at this layer) pointer to additional context that
  // EVP_PKEY functions may store in this object.
  EVP_PKEY_CTX *pctx;

  // pctx_ops, if not NULL, points to a vtable that contains functions to
  // manipulate |pctx|.
  const struct evp_md_pctx_ops *pctx_ops;

  // flags is only used for two cases.
  // 1. Set flag |EVP_MD_CTX_FLAG_KEEP_PKEY_CTX|, so as to let |*pctx| refrain
  //    from being freed when |*pctx| was set externally with
  //    |EVP_MD_CTX_set_pkey_ctx|.
  // 2. Set flag |EVP_MD_CTX_HMAC| for |EVP_PKEY_HMAC|.
  unsigned long flags;
} /* EVP_MD_CTX */;


// General No-op Functions [Deprecated].

// EVP_MD_unstable_sha3_enable is a no-op as SHA3 is always enabled.
OPENSSL_EXPORT OPENSSL_DEPRECATED void EVP_MD_unstable_sha3_enable(bool enable);

// EVP_MD_unstable_sha3_is_enabled always returns true as SHA3 is always
// enabled.
OPENSSL_EXPORT OPENSSL_DEPRECATED bool EVP_MD_unstable_sha3_is_enabled(void);

// EVP_MD_CTX_set_flags does nothing. We strongly discourage doing any
// additional configurations when consuming |EVP_MD_CTX|.
OPENSSL_EXPORT OPENSSL_DEPRECATED void EVP_MD_CTX_set_flags(EVP_MD_CTX *ctx,
                                                            int flags);

// EVP_MD_CTX_FLAG_NON_FIPS_ALLOW is meaningless. In OpenSSL it permits non-FIPS
// algorithms in FIPS mode. But BoringSSL FIPS mode doesn't prohibit algorithms
// (it's up the the caller to use the FIPS module in a fashion compliant with
// their needs). Thus this exists only to allow code to compile.
#define EVP_MD_CTX_FLAG_NON_FIPS_ALLOW 0

// EVP_add_digest does nothing and returns one. It exists only for
// compatibility with OpenSSL, which requires manually loading supported digests
// when certain options are turned on.
OPENSSL_EXPORT OPENSSL_DEPRECATED int EVP_add_digest(const EVP_MD *digest);

// EVP_md_null is a "null" message digest that does nothing: i.e. the hash it
// returns is of zero length. Included for OpenSSL compatibility
OPENSSL_EXPORT OPENSSL_DEPRECATED const EVP_MD *EVP_md_null(void);


#if defined(__cplusplus)
}  // extern C

#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(EVP_MD_CTX, EVP_MD_CTX_free)

using ScopedEVP_MD_CTX =
    internal::StackAllocatedMovable<EVP_MD_CTX, int, EVP_MD_CTX_init,
                                    EVP_MD_CTX_cleanup, EVP_MD_CTX_move>;

BSSL_NAMESPACE_END

}  // extern C++
#endif

#endif

#define DIGEST_R_INPUT_NOT_INITIALIZED 100
#define DIGEST_R_DECODE_ERROR 101
#define DIGEST_R_UNKNOWN_HASH 102

#endif  // OPENSSL_HEADER_DIGEST_H
