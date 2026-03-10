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
 * [including the GNU Public Licence.]
 *
 * The DSS routines are based on patches supplied by
 * Steven Schoch <schoch@sheba.arc.nasa.gov>. */

#ifndef OPENSSL_HEADER_DSA_H
#define OPENSSL_HEADER_DSA_H

#include <openssl/base.h>
#include <openssl/crypto.h>

#include <openssl/ex_data.h>
#include <stdio.h>

#if defined(__cplusplus)
extern "C" {
#endif

#define OPENSSL_DSA_MAX_MODULUS_BITS 10000


// DSA contains functions for signing and verifying with the Digital Signature
// Algorithm.
//
// This module is deprecated and retained for legacy reasons only. It is not
// considered a priority for performance or hardening work. Do not use it in
// new code. Use Ed25519, ECDSA with P-256, or RSA instead.


// Allocation and destruction.
//
// A |DSA| object represents a DSA key or group parameters. A given object may
// be used concurrently on multiple threads by non-mutating functions, provided
// no other thread is concurrently calling a mutating function. Unless otherwise
// documented, functions which take a |const| pointer are non-mutating and
// functions which take a non-|const| pointer are mutating.

// DSA_new returns a new, empty DSA object or NULL on error.
OPENSSL_EXPORT DSA *DSA_new(void);

// DSA_free decrements the reference count of |dsa| and frees it if the
// reference count drops to zero.
OPENSSL_EXPORT void DSA_free(DSA *dsa);

// DSA_up_ref increments the reference count of |dsa| and returns one. It does
// not mutate |dsa| for thread-safety purposes and may be used concurrently.
OPENSSL_EXPORT int DSA_up_ref(DSA *dsa);

// DSA_print prints a textual representation of |dsa| to |bio|. It returns one
// on success or zero otherwise.
OPENSSL_EXPORT int DSA_print(BIO *bio, const DSA *dsa, int indent);

// DSA_print_fp prints a textual representation of |dsa| to |fp|. It returns one
// on success or zero otherwise.
OPENSSL_EXPORT int DSA_print_fp(FILE *fp, const DSA *dsa, int indent);

// Properties.

// DSA_bits returns the size of |dsa|'s group modulus, in bits.
OPENSSL_EXPORT unsigned DSA_bits(const DSA *dsa);

// DSA_get0_pub_key returns |dsa|'s public key.
OPENSSL_EXPORT const BIGNUM *DSA_get0_pub_key(const DSA *dsa);

// DSA_get0_priv_key returns |dsa|'s private key, or NULL if |dsa| is a public
// key.
OPENSSL_EXPORT const BIGNUM *DSA_get0_priv_key(const DSA *dsa);

// DSA_get0_p returns |dsa|'s group modulus.
OPENSSL_EXPORT const BIGNUM *DSA_get0_p(const DSA *dsa);

// DSA_get0_q returns the size of |dsa|'s subgroup.
OPENSSL_EXPORT const BIGNUM *DSA_get0_q(const DSA *dsa);

// DSA_get0_g returns |dsa|'s group generator.
OPENSSL_EXPORT const BIGNUM *DSA_get0_g(const DSA *dsa);

// DSA_get0_key sets |*out_pub_key| and |*out_priv_key|, if non-NULL, to |dsa|'s
// public and private key, respectively. If |dsa| is a public key, the private
// key will be set to NULL.
OPENSSL_EXPORT void DSA_get0_key(const DSA *dsa, const BIGNUM **out_pub_key,
                                 const BIGNUM **out_priv_key);

// DSA_get0_pqg sets |*out_p|, |*out_q|, and |*out_g|, if non-NULL, to |dsa|'s
// p, q, and g parameters, respectively.
OPENSSL_EXPORT void DSA_get0_pqg(const DSA *dsa, const BIGNUM **out_p,
                                 const BIGNUM **out_q, const BIGNUM **out_g);

// DSA_set0_key sets |dsa|'s public and private key to |pub_key| and |priv_key|,
// respectively, if non-NULL. On success, it takes ownership of each argument
// and returns one. Otherwise, it returns zero.
//
// |priv_key| may be NULL, but |pub_key| must either be non-NULL or already
// configured on |dsa|.
OPENSSL_EXPORT int DSA_set0_key(DSA *dsa, BIGNUM *pub_key, BIGNUM *priv_key);

// DSA_set0_pqg sets |dsa|'s parameters to |p|, |q|, and |g|, if non-NULL, and
// takes ownership of them. On success, it takes ownership of each argument and
// returns one. Otherwise, it returns zero.
//
// Each argument must either be non-NULL or already configured on |dsa|.
OPENSSL_EXPORT int DSA_set0_pqg(DSA *dsa, BIGNUM *p, BIGNUM *q, BIGNUM *g);


// Parameter generation.

// DSA_generate_parameters_ex generates a set of DSA parameters by following
// the procedure given in FIPS 186-4, appendix A.
// (http://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.186-4.pdf)
//
// The larger prime will have a length of |bits| (e.g. 2048). The |seed| value
// allows others to generate and verify the same parameters and should be
// random input which is kept for reference. If |out_counter| or |out_h| are
// not NULL then the counter and h value used in the generation are written to
// them.
//
// The |cb| argument is passed to |BN_generate_prime_ex| and is thus called
// during the generation process in order to indicate progress. See the
// comments for that function for details. In addition to the calls made by
// |BN_generate_prime_ex|, |DSA_generate_parameters_ex| will call it with
// |event| equal to 2 and 3 at different stages of the process.
//
// It returns one on success and zero otherwise.
OPENSSL_EXPORT int DSA_generate_parameters_ex(DSA *dsa, unsigned bits,
                                              const uint8_t *seed,
                                              size_t seed_len, int *out_counter,
                                              unsigned long *out_h,
                                              BN_GENCB *cb);

// DSAparams_dup returns a freshly allocated |DSA| that contains a copy of the
// parameters from |dsa|. It returns NULL on error.
OPENSSL_EXPORT DSA *DSAparams_dup(const DSA *dsa);


// Key generation.

// DSA_generate_key generates a public/private key pair in |dsa|, which must
// already have parameters setup. Only supports generating up to |OPENSSL_DSA_MAX_MODULUS_BITS|
// bit keys. It returns one on success and zero on error.
OPENSSL_EXPORT int DSA_generate_key(DSA *dsa);


// Signatures.

// DSA_SIG_st (aka |DSA_SIG|) contains a DSA signature as a pair of integers.
struct DSA_SIG_st {
  BIGNUM *r, *s;
};

// DSA_SIG_new returns a freshly allocated, DIG_SIG structure or NULL on error.
// Both |r| and |s| in the signature will be NULL.
OPENSSL_EXPORT DSA_SIG *DSA_SIG_new(void);

// DSA_SIG_free frees the contents of |sig| and then frees |sig| itself.
OPENSSL_EXPORT void DSA_SIG_free(DSA_SIG *sig);

// DSA_SIG_get0 sets |*out_r| and |*out_s|, if non-NULL, to the two components
// of |sig|.
OPENSSL_EXPORT void DSA_SIG_get0(const DSA_SIG *sig, const BIGNUM **out_r,
                                 const BIGNUM **out_s);

// DSA_SIG_set0 sets |sig|'s components to |r| and |s|, neither of which may be
// NULL. On success, it takes ownership of each argument and returns one.
// Otherwise, it returns zero.
OPENSSL_EXPORT int DSA_SIG_set0(DSA_SIG *sig, BIGNUM *r, BIGNUM *s);

// DSA_do_sign returns a signature of the hash in |digest| by the key in |dsa|
// and returns an allocated, DSA_SIG structure, or NULL on error.
OPENSSL_EXPORT DSA_SIG *DSA_do_sign(const uint8_t *digest, size_t digest_len,
                                    const DSA *dsa);

// DSA_do_verify verifies that |sig| is a valid signature, by the public key in
// |dsa|, of the hash in |digest|. It returns one if so, zero if invalid and -1
// on error.
//
// WARNING: do not use. This function returns -1 for error, 0 for invalid and 1
// for valid. However, this is dangerously different to the usual OpenSSL
// convention and could be a disaster if a user did |if (DSA_do_verify(...))|.
// Because of this, |DSA_check_signature| is a safer version of this.
//
// TODO(fork): deprecate.
OPENSSL_EXPORT int DSA_do_verify(const uint8_t *digest, size_t digest_len,
                                 const DSA_SIG *sig, const DSA *dsa);

// DSA_do_check_signature sets |*out_valid| to zero. Then it verifies that |sig|
// is a valid signature, by the public key in |dsa| of the hash in |digest|
// and, if so, it sets |*out_valid| to one.
//
// It returns one if it was able to verify the signature as valid or invalid,
// and zero on error.
OPENSSL_EXPORT int DSA_do_check_signature(int *out_valid, const uint8_t *digest,
                                          size_t digest_len, const DSA_SIG *sig,
                                          const DSA *dsa);


// ASN.1 signatures.
//
// These functions also perform DSA signature operations, but deal with ASN.1
// encoded signatures as opposed to raw |BIGNUM|s. If you don't know what
// encoding a DSA signature is in, it's probably ASN.1.

// DSA_sign signs |digest| with the key in |dsa| and writes the resulting
// signature, in ASN.1 form, to |out_sig| and the length of the signature to
// |*out_siglen|. There must be, at least, |DSA_size(dsa)| bytes of space in
// |out_sig|. It returns one on success and zero otherwise.
//
// (The |type| argument is ignored.)
OPENSSL_EXPORT int DSA_sign(int type, const uint8_t *digest, size_t digest_len,
                            uint8_t *out_sig, unsigned int *out_siglen,
                            const DSA *dsa);

// DSA_verify verifies that |sig| is a valid, ASN.1 signature, by the public
// key in |dsa|, of the hash in |digest|. It returns one if so, zero if invalid
// and -1 on error.
//
// (The |type| argument is ignored.)
//
// WARNING: do not use. This function returns -1 for error, 0 for invalid and 1
// for valid. However, this is dangerously different to the usual OpenSSL
// convention and could be a disaster if a user did |if (DSA_do_verify(...))|.
// Because of this, |DSA_check_signature| is a safer version of this.
//
// TODO(fork): deprecate.
OPENSSL_EXPORT int DSA_verify(int type, const uint8_t *digest,
                              size_t digest_len, const uint8_t *sig,
                              size_t sig_len, const DSA *dsa);

// DSA_check_signature sets |*out_valid| to zero. Then it verifies that |sig|
// is a valid, ASN.1 signature, by the public key in |dsa|, of the hash in
// |digest|. If so, it sets |*out_valid| to one.
//
// It returns one if it was able to verify the signature as valid or invalid,
// and zero on error.
OPENSSL_EXPORT int DSA_check_signature(int *out_valid, const uint8_t *digest,
                                       size_t digest_len, const uint8_t *sig,
                                       size_t sig_len, const DSA *dsa);

// DSA_size returns the size, in bytes, of an ASN.1 encoded, DSA signature
// generated by |dsa|. Parameters must already have been setup in |dsa|.
OPENSSL_EXPORT int DSA_size(const DSA *dsa);


// ASN.1 encoding.

// DSA_SIG_parse parses a DER-encoded DSA-Sig-Value structure from |cbs| and
// advances |cbs|. It returns a newly-allocated |DSA_SIG| or NULL on error.
OPENSSL_EXPORT DSA_SIG *DSA_SIG_parse(CBS *cbs);

// DSA_SIG_marshal marshals |sig| as a DER-encoded DSA-Sig-Value and appends the
// result to |cbb|. It returns one on success and zero on error.
OPENSSL_EXPORT int DSA_SIG_marshal(CBB *cbb, const DSA_SIG *sig);

// DSA_parse_public_key parses a DER-encoded DSA public key from |cbs| and
// advances |cbs|. It returns a newly-allocated |DSA| or NULL on error.
OPENSSL_EXPORT DSA *DSA_parse_public_key(CBS *cbs);

// DSA_marshal_public_key marshals |dsa| as a DER-encoded DSA public key and
// appends the result to |cbb|. It returns one on success and zero on
// failure.
OPENSSL_EXPORT int DSA_marshal_public_key(CBB *cbb, const DSA *dsa);

// DSA_parse_private_key parses a DER-encoded DSA private key from |cbs| and
// advances |cbs|. It returns a newly-allocated |DSA| or NULL on error.
OPENSSL_EXPORT DSA *DSA_parse_private_key(CBS *cbs);

// DSA_marshal_private_key marshals |dsa| as a DER-encoded DSA private key and
// appends the result to |cbb|. It returns one on success and zero on
// failure.
OPENSSL_EXPORT int DSA_marshal_private_key(CBB *cbb, const DSA *dsa);

// DSA_parse_parameters parses a DER-encoded Dss-Parms structure (RFC 3279)
// from |cbs| and advances |cbs|. It returns a newly-allocated |DSA| or NULL on
// error.
OPENSSL_EXPORT DSA *DSA_parse_parameters(CBS *cbs);

// DSA_marshal_parameters marshals |dsa| as a DER-encoded Dss-Parms structure
// (RFC 3279) and appends the result to |cbb|. It returns one on success and
// zero on failure.
OPENSSL_EXPORT int DSA_marshal_parameters(CBB *cbb, const DSA *dsa);


// Conversion.

// DSA_dup_DH returns a |DH| constructed from the parameters of |dsa|. This is
// sometimes needed when Diffie-Hellman parameters are stored in the form of
// DSA parameters. It returns an allocated |DH| on success or NULL on error.
OPENSSL_EXPORT DH *DSA_dup_DH(const DSA *dsa);


// ex_data functions.
//
// See |ex_data.h| for details.

OPENSSL_EXPORT int DSA_get_ex_new_index(long argl, void *argp,
                                        CRYPTO_EX_unused *unused,
                                        CRYPTO_EX_dup *dup_unused,
                                        CRYPTO_EX_free *free_func);
OPENSSL_EXPORT int DSA_set_ex_data(DSA *dsa, int idx, void *arg);
OPENSSL_EXPORT void *DSA_get_ex_data(const DSA *dsa, int idx);


// Deprecated functions.

// d2i_DSA_SIG parses a DER-encoded DSA-Sig-Value structure from |len| bytes at
// |*inp|, as described in |d2i_SAMPLE|.
//
// Use |DSA_SIG_parse| instead.
OPENSSL_EXPORT DSA_SIG *d2i_DSA_SIG(DSA_SIG **out_sig, const uint8_t **inp,
                                    long len);

// i2d_DSA_SIG marshals |in| to a DER-encoded DSA-Sig-Value structure, as
// described in |i2d_SAMPLE|.
//
// Use |DSA_SIG_marshal| instead.
OPENSSL_EXPORT int i2d_DSA_SIG(const DSA_SIG *in, uint8_t **outp);

// d2i_DSAPublicKey parses a DER-encoded DSA public key from |len| bytes at
// |*inp|, as described in |d2i_SAMPLE|.
//
// Use |DSA_parse_public_key| instead.
OPENSSL_EXPORT DSA *d2i_DSAPublicKey(DSA **out, const uint8_t **inp, long len);

// i2d_DSAPublicKey marshals |in| as a DER-encoded DSA public key, as described
// in |i2d_SAMPLE|.
//
// Use |DSA_marshal_public_key| instead.
OPENSSL_EXPORT int i2d_DSAPublicKey(const DSA *in, uint8_t **outp);

// d2i_DSAPrivateKey parses a DER-encoded DSA private key from |len| bytes at
// |*inp|, as described in |d2i_SAMPLE|.
//
// Use |DSA_parse_private_key| instead.
OPENSSL_EXPORT DSA *d2i_DSAPrivateKey(DSA **out, const uint8_t **inp, long len);

// i2d_DSAPrivateKey marshals |in| as a DER-encoded DSA private key, as
// described in |i2d_SAMPLE|.
//
// Use |DSA_marshal_private_key| instead.
OPENSSL_EXPORT int i2d_DSAPrivateKey(const DSA *in, uint8_t **outp);

// d2i_DSAparams parses a DER-encoded Dss-Parms structure (RFC 3279) from |len|
// bytes at |*inp|, as described in |d2i_SAMPLE|.
//
// Use |DSA_parse_parameters| instead.
OPENSSL_EXPORT DSA *d2i_DSAparams(DSA **out, const uint8_t **inp, long len);

// i2d_DSAparams marshals |in|'s parameters as a DER-encoded Dss-Parms structure
// (RFC 3279), as described in |i2d_SAMPLE|.
//
// Use |DSA_marshal_parameters| instead.
OPENSSL_EXPORT int i2d_DSAparams(const DSA *in, uint8_t **outp);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(DSA, DSA_free)
BORINGSSL_MAKE_UP_REF(DSA, DSA_up_ref)
BORINGSSL_MAKE_DELETER(DSA_SIG, DSA_SIG_free)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#define DSA_R_BAD_Q_VALUE 100
#define DSA_R_MISSING_PARAMETERS 101
#define DSA_R_MODULUS_TOO_LARGE 102
#define DSA_R_NEED_NEW_SETUP_VALUES 103
#define DSA_R_BAD_VERSION 104
#define DSA_R_DECODE_ERROR 105
#define DSA_R_ENCODE_ERROR 106
#define DSA_R_INVALID_PARAMETERS 107
#define DSA_R_TOO_MANY_ITERATIONS 108

#endif  // OPENSSL_HEADER_DSA_H
