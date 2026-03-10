/* Originally written by Bodo Moeller for the OpenSSL project.
 * ====================================================================
 * Copyright (c) 1998-2005 The OpenSSL Project.  All rights reserved.
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
 * The Contribution is licensed pursuant to the OpenSSL open source
 * license provided above.
 *
 * The elliptic curve binary polynomial software is originally written by
 * Sheueling Chang Shantz and Douglas Stebila of Sun Microsystems
 * Laboratories. */

#ifndef OPENSSL_HEADER_EC_KEY_H
#define OPENSSL_HEADER_EC_KEY_H

#include <openssl/base.h>

#include <openssl/ec.h>
#include <openssl/engine.h>
#include <openssl/ex_data.h>

#if defined(__cplusplus)
extern "C" {
#endif


// ec_key.h contains functions that handle elliptic-curve points that are
// public/private keys.


// EC key objects.
//
// An |EC_KEY| object represents a public or private EC key. A given object may
// be used concurrently on multiple threads by non-mutating functions, provided
// no other thread is concurrently calling a mutating function. Unless otherwise
// documented, functions which take a |const| pointer are non-mutating and
// functions which take a non-|const| pointer are mutating.

// EC_KEY_new returns a fresh |EC_KEY| object or NULL on error.
OPENSSL_EXPORT EC_KEY *EC_KEY_new(void);

// EC_KEY_new_method acts the same as |EC_KEY_new|, but takes an explicit
// |ENGINE|.
OPENSSL_EXPORT EC_KEY *EC_KEY_new_method(const ENGINE *engine);

// EC_KEY_new_by_curve_name returns a fresh EC_KEY for group specified by |nid|
// or NULL on error.
OPENSSL_EXPORT EC_KEY *EC_KEY_new_by_curve_name(int nid);

// EC_KEY_free frees all the data owned by |key| and |key| itself.
OPENSSL_EXPORT void EC_KEY_free(EC_KEY *key);

// EC_KEY_dup returns a fresh copy of |src| or NULL on error.
OPENSSL_EXPORT EC_KEY *EC_KEY_dup(const EC_KEY *src);

// EC_KEY_up_ref increases the reference count of |key| and returns one. It does
// not mutate |key| for thread-safety purposes and may be used concurrently.
OPENSSL_EXPORT int EC_KEY_up_ref(EC_KEY *key);

// EC_KEY_is_opaque returns one if |key| is opaque and doesn't expose its key
// material. Otherwise it return zero.
OPENSSL_EXPORT int EC_KEY_is_opaque(const EC_KEY *key);

// EC_KEY_get0_group returns a pointer to the |EC_GROUP| object inside |key|.
OPENSSL_EXPORT const EC_GROUP *EC_KEY_get0_group(const EC_KEY *key);

// EC_KEY_set_group sets the |EC_GROUP| object that |key| will use to |group|.
// It returns one on success and zero if |key| is already configured with a
// different group.
OPENSSL_EXPORT int EC_KEY_set_group(EC_KEY *key, const EC_GROUP *group);

// EC_KEY_get0_private_key returns a pointer to the private key inside |key|.
OPENSSL_EXPORT const BIGNUM *EC_KEY_get0_private_key(const EC_KEY *key);

// EC_KEY_set_private_key sets the private key of |key| to |priv|. It returns
// one on success and zero otherwise. |key| must already have had a group
// configured (see |EC_KEY_set_group| and |EC_KEY_new_by_curve_name|).
OPENSSL_EXPORT int EC_KEY_set_private_key(EC_KEY *key, const BIGNUM *priv);

// EC_KEY_get0_public_key returns a pointer to the public key point inside
// |key|.
OPENSSL_EXPORT const EC_POINT *EC_KEY_get0_public_key(const EC_KEY *key);

// EC_KEY_set_public_key sets the public key of |key| to |pub|, by copying it.
// It returns one on success and zero otherwise. |key| must already have had a
// group configured (see |EC_KEY_set_group| and |EC_KEY_new_by_curve_name|), and
// |pub| must also belong to that group.
OPENSSL_EXPORT int EC_KEY_set_public_key(EC_KEY *key, const EC_POINT *pub);

#define EC_PKEY_NO_PARAMETERS 0x001
#define EC_PKEY_NO_PUBKEY 0x002

// EC_KEY_get_enc_flags returns the encoding flags for |key|, which is a
// bitwise-OR of |EC_PKEY_*| values.
OPENSSL_EXPORT unsigned EC_KEY_get_enc_flags(const EC_KEY *key);

// EC_KEY_set_enc_flags sets the encoding flags for |key|, which is a
// bitwise-OR of |EC_PKEY_*| values.
OPENSSL_EXPORT void EC_KEY_set_enc_flags(EC_KEY *key, unsigned flags);

// EC_KEY_get_conv_form returns the conversation form that will be used by
// |key|.
OPENSSL_EXPORT point_conversion_form_t EC_KEY_get_conv_form(const EC_KEY *key);

// EC_KEY_set_conv_form sets the conversion form to be used by |key|.
OPENSSL_EXPORT void EC_KEY_set_conv_form(EC_KEY *key,
                                         point_conversion_form_t cform);

// EC_KEY_check_key performs several checks on |key| including, if the
// private is present, an expensive check that the public key
// corresponds to it. It returns one if all checks pass and zero
// otherwise. If it returns zero then detail about the problem can be
// found on the error stack.
OPENSSL_EXPORT int EC_KEY_check_key(const EC_KEY *key);

// EC_KEY_check_fips performs a signing pairwise consistency test (FIPS 140-2
// 4.9.2) and the consistency test from SP 800-56Ar3 section 5.6.2.1.4.
// If the public key contains an affine point, it also checks that its
// coordinates are in the range [0, p-1]. That is in addition to the checks
// performed by EC_KEY_check_key.
// It returns one if it passes and zero otherwise.
OPENSSL_EXPORT int EC_KEY_check_fips(const EC_KEY *key);

// EC_KEY_set_public_key_affine_coordinates sets the public key in |key| to
// (|x|, |y|). It returns one on success and zero on error. It's considered an
// error if |x| and |y| do not represent a point on |key|'s curve.
OPENSSL_EXPORT int EC_KEY_set_public_key_affine_coordinates(EC_KEY *key,
                                                            const BIGNUM *x,
                                                            const BIGNUM *y);

// EC_KEY_key2buf encodes the public key in |key| to an allocated octet string
// and sets |*out_buf| to point to it. It returns the length of the encoded
// octet string or zero if an error occurred.
OPENSSL_EXPORT size_t EC_KEY_key2buf(const EC_KEY *key,
                                     point_conversion_form_t form,
                                     unsigned char **out_buf, BN_CTX *ctx);


// Key generation.

// EC_KEY_generate_key generates a random, private key, calculates the
// corresponding public key and stores both in |key|. It returns one on success
// or zero otherwise.
OPENSSL_EXPORT int EC_KEY_generate_key(EC_KEY *key);

// EC_KEY_generate_key_fips behaves like |EC_KEY_generate_key| but performs
// additional checks for FIPS compliance. This function is applicable when
// generating keys for either signing/verification or key agreement because
// both types of consistency check (PCT) are performed.
OPENSSL_EXPORT int EC_KEY_generate_key_fips(EC_KEY *key);

// EC_KEY_derive_from_secret deterministically derives a private key for |group|
// from an input secret using HKDF-SHA256. It returns a newly-allocated |EC_KEY|
// on success or NULL on error. |secret| must not be used in any other
// algorithm. If using a base secret for multiple operations, derive separate
// values with a KDF such as HKDF first.
//
// Note this function implements an arbitrary derivation scheme, rather than any
// particular standard one. New protocols are recommended to use X25519 and
// Ed25519, which have standard byte import functions. See
// |X25519_public_from_private| and |ED25519_keypair_from_seed|.
OPENSSL_EXPORT EC_KEY *EC_KEY_derive_from_secret(const EC_GROUP *group,
                                                 const uint8_t *secret,
                                                 size_t secret_len);


// Serialisation.

// EC_KEY_parse_private_key parses a DER-encoded ECPrivateKey structure (RFC
// 5915) from |cbs| and advances |cbs|. It returns a newly-allocated |EC_KEY| or
// NULL on error. If |group| is non-null, the parameters field of the
// ECPrivateKey may be omitted (but must match |group| if present). Otherwise,
// the parameters field is required.
OPENSSL_EXPORT EC_KEY *EC_KEY_parse_private_key(CBS *cbs,
                                                const EC_GROUP *group);

// EC_KEY_marshal_private_key marshals |key| as a DER-encoded ECPrivateKey
// structure (RFC 5915) and appends the result to |cbb|. It returns one on
// success and zero on failure. |enc_flags| is a combination of |EC_PKEY_*|
// values and controls whether corresponding fields are omitted.
OPENSSL_EXPORT int EC_KEY_marshal_private_key(CBB *cbb, const EC_KEY *key,
                                              unsigned enc_flags);

// EC_KEY_parse_curve_name parses a DER-encoded OBJECT IDENTIFIER as a curve
// name from |cbs| and advances |cbs|. It returns the decoded |EC_GROUP| or NULL
// on error.
//
// This function returns a non-const pointer which may be passed to
// |EC_GROUP_free|. However, the resulting object is actually static and calling
// |EC_GROUP_free| is optional.
//
// TODO(davidben): Make this return a const pointer, if it does not break too
// many callers.
OPENSSL_EXPORT EC_GROUP *EC_KEY_parse_curve_name(CBS *cbs);

// EC_KEY_marshal_curve_name marshals |group| as a DER-encoded OBJECT IDENTIFIER
// and appends the result to |cbb|. It returns one on success and zero on
// failure.
OPENSSL_EXPORT int EC_KEY_marshal_curve_name(CBB *cbb, const EC_GROUP *group);

// EC_KEY_parse_parameters parses a DER-encoded ECParameters structure (RFC
// 5480) from |cbs| and advances |cbs|. It returns the resulting |EC_GROUP| or
// NULL on error. It supports the namedCurve and specifiedCurve options, but use
// of specifiedCurve is deprecated. Use |EC_KEY_parse_curve_name| instead.
//
// This function returns a non-const pointer which may be passed to
// |EC_GROUP_free|. However, the resulting object is actually static and calling
// |EC_GROUP_free| is optional.
//
// TODO(davidben): Make this return a const pointer, if it does not break too
// many callers.
OPENSSL_EXPORT EC_GROUP *EC_KEY_parse_parameters(CBS *cbs);


// ex_data functions.
//
// These functions are wrappers. See |ex_data.h| for details.

OPENSSL_EXPORT int EC_KEY_get_ex_new_index(long argl, void *argp,
                                           CRYPTO_EX_unused *unused,
                                           CRYPTO_EX_dup *dup_unused,
                                           CRYPTO_EX_free *free_func);
OPENSSL_EXPORT int EC_KEY_set_ex_data(EC_KEY *r, int idx, void *arg);
OPENSSL_EXPORT void *EC_KEY_get_ex_data(const EC_KEY *r, int idx);


// Deprecated functions.

// d2i_ECPrivateKey parses a DER-encoded ECPrivateKey structure (RFC 5915) from
// |len| bytes at |*inp|, as described in |d2i_SAMPLE|. On input, if |*out_key|
// is non-NULL and has a group configured, the parameters field may be omitted
// but must match that group if present.
//
// Use |EC_KEY_parse_private_key| instead.
OPENSSL_EXPORT EC_KEY *d2i_ECPrivateKey(EC_KEY **out_key, const uint8_t **inp,
                                        long len);

// i2d_ECPrivateKey marshals |key| as a DER-encoded ECPrivateKey structure (RFC
// 5915), as described in |i2d_SAMPLE|.
//
// Use |EC_KEY_marshal_private_key| instead.
OPENSSL_EXPORT int i2d_ECPrivateKey(const EC_KEY *key, uint8_t **outp);

// d2i_ECParameters parses a DER-encoded ECParameters structure (RFC 5480) from
// |len| bytes at |*inp|, as described in |d2i_SAMPLE|.
//
// Use |EC_KEY_parse_parameters| or |EC_KEY_parse_curve_name| instead. Only
// deserialization of namedCurves or explicitly-encoded versions of named curves
// are supported.
OPENSSL_EXPORT EC_KEY *d2i_ECParameters(EC_KEY **out_key, const uint8_t **inp,
                                        long len);

// i2d_ECParameters marshals |key|'s parameters as a DER-encoded OBJECT
// IDENTIFIER, as described in |i2d_SAMPLE|.
//
// Use |EC_KEY_marshal_curve_name| instead. Only serialization of namedCurves
// are supported.
OPENSSL_EXPORT int i2d_ECParameters(const EC_KEY *key, uint8_t **outp);

// d2i_ECPKParameters_bio deserializes the |ECPKParameters| specified in RFC
// 3279 from |bio| and returns the corresponding |EC_GROUP|. If |*out_group| is
// non-null, the original |*out_group| is freed and the returned |EC_GROUP| is
// also written to |*out_group|. The user continues to maintain the memory
// assigned to |*out_group| if non-null.
//
// Only deserialization of namedCurves or
// explicitly-encoded versions of namedCurves are supported.
OPENSSL_EXPORT EC_GROUP *d2i_ECPKParameters_bio(BIO *bio, EC_GROUP **out_group);

// i2d_ECPKParameters_bio serializes an |EC_GROUP| to |bio| according to the
// |ECPKParameters| specified in RFC 3279. It returns 1 on success and 0 on
// failure.
// Only serialization of namedCurves are supported.
OPENSSL_EXPORT int i2d_ECPKParameters_bio(BIO *bio, const EC_GROUP *group);

// o2i_ECPublicKey parses an EC point from |len| bytes at |*inp| into
// |*out_key|. Note that this differs from the d2i format in that |*out_key|
// must be non-NULL with a group set. On successful exit, |*inp| is advanced by
// |len| bytes. It returns |*out_key| or NULL on error.
//
// Use |EC_POINT_oct2point| instead.
OPENSSL_EXPORT EC_KEY *o2i_ECPublicKey(EC_KEY **out_key, const uint8_t **inp,
                                       long len);

// i2o_ECPublicKey marshals an EC point from |key|, as described in
// |i2d_SAMPLE|, except it returns zero on error instead of a negative value.
//
// Use |EC_POINT_point2cbb| instead.
OPENSSL_EXPORT int i2o_ECPublicKey(const EC_KEY *key, unsigned char **outp);


// EC_KEY_METHOD
// This struct replaces the old |ECDSA_METHOD| struct.

// ECDSA_FLAG_OPAQUE specifies that this EC_KEY_METHOD does not expose its key
// material. This may be set if, for instance, it is wrapping some other crypto
// API, like a platform key store. Use |EC_KEY_METHOD_set_flag| to set
// this flag on an |EC_KEY_METHOD|. It is not set by default.
// This was supported in ECDSA_METHOD previously.
#define ECDSA_FLAG_OPAQUE 1

// EC_KEY_get_default_method returns a reference to the default
// |EC_KEY| implementation. All |EC_KEY| objects are initialized with the
// returned struct. This function currently calls |EC_KEY_OpenSSL| since AWS-LC
// does not support changing/setting the default method.
OPENSSL_EXPORT const EC_KEY_METHOD *EC_KEY_get_default_method(void);

// EC_KEY_OpenSSL returns a reference to the default |EC_KEY| implementation.
// The returned |EC_KEY_METHOD| object is statically allocated. The application
// should not free this struct.
//
// This struct is also zero-initialized. This is different from OpenSSL which
// returns function pointers to the default implementations within the
// |EC_KEY_METHOD| struct. We do not do this to make it easier for the
// compiler/linker to drop unused functions. The wrapper functions for a given
// operation (e.g. |ECDSA_sign| corresponds to the |sign| field in
// |EC_KEY_METHOD|) will select the appropriate default implementation.
OPENSSL_EXPORT const EC_KEY_METHOD *EC_KEY_OpenSSL(void);

// EC_KEY_METHOD_new returns a newly allocated |EC_KEY_METHOD| object. If the
// input parameter |eckey_meth| is non-NULL, the function pointers within the
// returned |EC_KEY_METHOD| object will be initialized to the values from
// |eckey_meth|. If |eckey_meth| is NULL, the returned object will be
// initialized using the value returned from |EC_KEY_get_default_method|.
OPENSSL_EXPORT EC_KEY_METHOD *EC_KEY_METHOD_new(
    const EC_KEY_METHOD *eckey_meth);

// EC_KEY_METHOD_free frees the memory associated with |eckey_meth|
OPENSSL_EXPORT void EC_KEY_METHOD_free(EC_KEY_METHOD *eckey_meth);

// EC_KEY_set_method sets |meth| on |ec|. We do not support setting the
// |copy|, |set_group|, |set_private|, |set_public|, and |sign_setup|
// fields in |ec| and these pointers should be set to NULL. We do not support
// the |verify|, |verify_sig|, or |keygen| fields yet.
//
// Returns zero on failure and one on success.
OPENSSL_EXPORT int EC_KEY_set_method(EC_KEY *ec, const EC_KEY_METHOD *meth);

// EC_KEY_get_method returns the |EC_KEY_METHOD| object associated with |ec|.
OPENSSL_EXPORT const EC_KEY_METHOD *EC_KEY_get_method(const EC_KEY *ec);

// EC_KEY_METHOD_set_sign_awslc sets the |sign| and |sign_sig| pointers on
// |meth|.
OPENSSL_EXPORT void EC_KEY_METHOD_set_sign_awslc(
    EC_KEY_METHOD *meth,
    int (*sign)(int type, const uint8_t *digest, int digest_len, uint8_t *sig,
                unsigned int *siglen, const BIGNUM *k_inv, const BIGNUM *r,
                EC_KEY *eckey),
    ECDSA_SIG *(*sign_sig)(const uint8_t *digest, int digest_len,
                           const BIGNUM *in_kinv, const BIGNUM *in_r,
                           EC_KEY *eckey));


// EC_KEY_METHOD_set_sign sets function pointers on |meth|. AWS-LC currently
// supports setting |sign| and |sign_sig|. |sign_setup| must be set to NULL in
// order to compile with AWS-LC.
#define EC_KEY_METHOD_set_sign(meth, sign, sign_setup, sign_sig)      \
  OPENSSL_STATIC_ASSERT((sign_setup) == NULL,                         \
                        EC_KEY_METHOD_sign_setup_field_must_be_NULL); \
  EC_KEY_METHOD_set_sign_awslc(meth, sign, sign_sig);

// EC_KEY_METHOD_set_init_awslc sets the |init| and |finish| pointers on |meth|.
OPENSSL_EXPORT void EC_KEY_METHOD_set_init_awslc(EC_KEY_METHOD *meth,
                                                 int (*init)(EC_KEY *key),
                                                 void (*finish)(EC_KEY *key));


// EC_KEY_METHOD_set_init sets function pointers on |meth|. AWS-LC
// currently only supports setting the |init| and |finish| fields. |copy|,
// |set_group|, |set_private|, and |set_public| cannot be set yet and must
// be NULL.
#define EC_KEY_METHOD_set_init(meth, init, finish, copy, set_group,                 \
                               set_private, set_public)                             \
  OPENSSL_STATIC_ASSERT(                                                            \
      (copy) == NULL && (set_group) == NULL && (set_private) == NULL &&             \
          (set_public) == NULL,                                                     \
      EC_KEY_METHOD_copy_set_group_set_private_and_set_public_fields_must_be_NULL); \
  EC_KEY_METHOD_set_init_awslc(meth, init, finish);

// EC_KEY_METHOD_set_flags sets |flags| on |meth|. Currently, the only supported
// flag is |ECDSA_FLAG_OPAQUE|. Returns zero on failure and one on success.
OPENSSL_EXPORT int EC_KEY_METHOD_set_flags(EC_KEY_METHOD *meth, int flags);


// General No-op Functions [Deprecated].

// EC_KEY_set_asn1_flag does nothing. AWS-LC only supports
// |OPENSSL_EC_NAMED_CURVE|.
OPENSSL_EXPORT OPENSSL_DEPRECATED void EC_KEY_set_asn1_flag(EC_KEY *key,
                                                            int flag);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(EC_KEY, EC_KEY_free)
BORINGSSL_MAKE_UP_REF(EC_KEY, EC_KEY_up_ref)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#endif  // OPENSSL_HEADER_EC_KEY_H
