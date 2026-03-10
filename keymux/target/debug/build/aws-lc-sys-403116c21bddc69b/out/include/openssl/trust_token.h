/* Copyright (c) 2020, Google Inc.
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

#ifndef OPENSSL_HEADER_TRUST_TOKEN_H
#define OPENSSL_HEADER_TRUST_TOKEN_H

#include <openssl/base.h>
#include <openssl/stack.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Trust Token implementation.
//
// Trust Token is an implementation of an experimental mechanism similar to
// Privacy Pass which allows issuance and redemption of anonymized tokens with
// limited private metadata.
//
// References:
// https://eprint.iacr.org/2020/072.pdf
// https://github.com/alxdavids/privacy-pass-ietf/tree/master/drafts
// https://github.com/WICG/trust-token-api/blob/master/README.md
//
// WARNING: This API is unstable and subject to change.

// TRUST_TOKEN_experiment_v1 is an experimental Trust Tokens protocol using
// PMBTokens and P-384.
OPENSSL_EXPORT const TRUST_TOKEN_METHOD *TRUST_TOKEN_experiment_v1(void);

// TRUST_TOKEN_experiment_v2_voprf is an experimental Trust Tokens protocol
// using VOPRFs and P-384 with up to 6 keys, without RR verification.
OPENSSL_EXPORT const TRUST_TOKEN_METHOD *TRUST_TOKEN_experiment_v2_voprf(void);

// TRUST_TOKEN_experiment_v2_pmb is an experimental Trust Tokens protocol using
// PMBTokens and P-384 with up to 3 keys, without RR verification.
OPENSSL_EXPORT const TRUST_TOKEN_METHOD *TRUST_TOKEN_experiment_v2_pmb(void);

// TRUST_TOKEN_pst_v1_voprf is an experimental Trust Tokens protocol
// using VOPRFs and P-384 with up to 6 keys, without RR verification.
OPENSSL_EXPORT const TRUST_TOKEN_METHOD *TRUST_TOKEN_pst_v1_voprf(void);

// TRUST_TOKEN_pst_v1_pmb is an experimental Trust Tokens protocol using
// PMBTokens and P-384 with up to 3 keys, without RR verification.
OPENSSL_EXPORT const TRUST_TOKEN_METHOD *TRUST_TOKEN_pst_v1_pmb(void);

// trust_token_st represents a single-use token for the Trust Token protocol.
// For the client, this is the token and its corresponding signature. For the
// issuer, this is the token itself.
struct trust_token_st {
  uint8_t *data;
  size_t len;
};

DEFINE_STACK_OF(TRUST_TOKEN)

// TRUST_TOKEN_new creates a newly-allocated |TRUST_TOKEN| with value |data| or
// NULL on allocation failure.
OPENSSL_EXPORT TRUST_TOKEN *TRUST_TOKEN_new(const uint8_t *data, size_t len);

// TRUST_TOKEN_free releases memory associated with |token|.
OPENSSL_EXPORT void TRUST_TOKEN_free(TRUST_TOKEN *token);

#define TRUST_TOKEN_MAX_PRIVATE_KEY_SIZE 512
#define TRUST_TOKEN_MAX_PUBLIC_KEY_SIZE 512

// TRUST_TOKEN_generate_key creates a new Trust Token keypair labeled with |id|
// and serializes the private and public keys, writing the private key to
// |out_priv_key| and setting |*out_priv_key_len| to the number of bytes
// written, and writing the public key to |out_pub_key| and setting
// |*out_pub_key_len| to the number of bytes written.
//
// At most |max_priv_key_len| and |max_pub_key_len| bytes are written. In order
// to ensure success, these should be at least
// |TRUST_TOKEN_MAX_PRIVATE_KEY_SIZE| and |TRUST_TOKEN_MAX_PUBLIC_KEY_SIZE|.
//
// This function returns one on success or zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_generate_key(
    const TRUST_TOKEN_METHOD *method, uint8_t *out_priv_key,
    size_t *out_priv_key_len, size_t max_priv_key_len, uint8_t *out_pub_key,
    size_t *out_pub_key_len, size_t max_pub_key_len, uint32_t id);

// TRUST_TOKEN_derive_key_from_secret deterministically derives a new Trust
// Token keypair labeled with |id| from an input |secret| and serializes the
// private and public keys, writing the private key to |out_priv_key| and
// setting |*out_priv_key_len| to the number of bytes written, and writing the
// public key to |out_pub_key| and setting |*out_pub_key_len| to the number of
// bytes written.
//
// At most |max_priv_key_len| and |max_pub_key_len| bytes are written. In order
// to ensure success, these should be at least
// |TRUST_TOKEN_MAX_PRIVATE_KEY_SIZE| and |TRUST_TOKEN_MAX_PUBLIC_KEY_SIZE|.
//
// This function returns one on success or zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_derive_key_from_secret(
    const TRUST_TOKEN_METHOD *method, uint8_t *out_priv_key,
    size_t *out_priv_key_len, size_t max_priv_key_len, uint8_t *out_pub_key,
    size_t *out_pub_key_len, size_t max_pub_key_len, uint32_t id,
    const uint8_t *secret, size_t secret_len);


// Trust Token client implementation.
//
// These functions implements the client half of the Trust Token protocol. A
// single |TRUST_TOKEN_CLIENT| can perform a single protocol operation.

// TRUST_TOKEN_CLIENT_new returns a newly-allocated |TRUST_TOKEN_CLIENT|
// configured to use a max batchsize of |max_batchsize| or NULL on error.
// Issuance requests must be made in batches smaller than |max_batchsize|. This
// function will return an error if |max_batchsize| is too large for Trust
// Tokens.
OPENSSL_EXPORT TRUST_TOKEN_CLIENT *TRUST_TOKEN_CLIENT_new(
    const TRUST_TOKEN_METHOD *method, size_t max_batchsize);

// TRUST_TOKEN_CLIENT_free releases memory associated with |ctx|.
OPENSSL_EXPORT void TRUST_TOKEN_CLIENT_free(TRUST_TOKEN_CLIENT *ctx);

// TRUST_TOKEN_CLIENT_add_key configures the |ctx| to support the public key
// |key|. It sets |*out_key_index| to the index this key has been configured to.
// It returns one on success or zero on error if the |key| can't be parsed or
// too many keys have been configured.
OPENSSL_EXPORT int TRUST_TOKEN_CLIENT_add_key(TRUST_TOKEN_CLIENT *ctx,
                                              size_t *out_key_index,
                                              const uint8_t *key,
                                              size_t key_len);

// TRUST_TOKEN_CLIENT_set_srr_key sets the public key used to verify the SRR. It
// returns one on success and zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_CLIENT_set_srr_key(TRUST_TOKEN_CLIENT *ctx,
                                                  EVP_PKEY *key);

// TRUST_TOKEN_CLIENT_begin_issuance produces a request for |count| trust tokens
// and serializes the request into a newly-allocated buffer, setting |*out| to
// that buffer and |*out_len| to its length. The caller takes ownership of the
// buffer and must call |OPENSSL_free| when done. It returns one on success and
// zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_CLIENT_begin_issuance(TRUST_TOKEN_CLIENT *ctx,
                                                     uint8_t **out,
                                                     size_t *out_len,
                                                     size_t count);

// TRUST_TOKEN_CLIENT_begin_issuance_over_message produces a request for a trust
// token derived from |msg| and serializes the request into a newly-allocated
// buffer, setting |*out| to that buffer and |*out_len| to its length. The
// caller takes ownership of the buffer and must call |OPENSSL_free| when done.
// It returns one on success and zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_CLIENT_begin_issuance_over_message(
    TRUST_TOKEN_CLIENT *ctx, uint8_t **out, size_t *out_len, size_t count,
    const uint8_t *msg, size_t msg_len);

// TRUST_TOKEN_CLIENT_finish_issuance consumes |response| from the issuer and
// extracts the tokens, returning a list of tokens and the index of the key used
// to sign the tokens in |*out_key_index|. The caller can use this to determine
// what key was used in an issuance and to drop tokens if a new key commitment
// arrives without the specified key present. The caller takes ownership of the
// list and must call |sk_TRUST_TOKEN_pop_free| when done. The list is empty if
// issuance fails.
OPENSSL_EXPORT STACK_OF(TRUST_TOKEN) *
    TRUST_TOKEN_CLIENT_finish_issuance(TRUST_TOKEN_CLIENT *ctx,
                                       size_t *out_key_index,
                                       const uint8_t *response,
                                       size_t response_len);


// TRUST_TOKEN_CLIENT_begin_redemption produces a request to redeem a token
// |token| and receive a signature over |data| and serializes the request into
// a newly-allocated buffer, setting |*out| to that buffer and |*out_len| to
// its length. |time| is the number of seconds since the UNIX epoch and used to
// verify the validity of the issuer's response in TrustTokenV1 and ignored in
// other versions. The caller takes ownership of the buffer and must call
// |OPENSSL_free| when done. It returns one on success or zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_CLIENT_begin_redemption(
    TRUST_TOKEN_CLIENT *ctx, uint8_t **out, size_t *out_len,
    const TRUST_TOKEN *token, const uint8_t *data, size_t data_len,
    uint64_t time);

// TRUST_TOKEN_CLIENT_finish_redemption consumes |response| from the issuer. In
// |TRUST_TOKEN_experiment_v1|, it then verifies the SRR and if valid  sets
// |*out_rr| and |*out_rr_len| (respectively, |*out_sig| and |*out_sig_len|)
// to a newly-allocated buffer containing the SRR (respectively, the SRR
// signature). In other versions, it sets |*out_rr| and |*out_rr_len|
// to a newly-allocated buffer containing |response| and leaves all validation
// to the caller. It returns one on success or zero on failure.
OPENSSL_EXPORT int TRUST_TOKEN_CLIENT_finish_redemption(
    TRUST_TOKEN_CLIENT *ctx, uint8_t **out_rr, size_t *out_rr_len,
    uint8_t **out_sig, size_t *out_sig_len, const uint8_t *response,
    size_t response_len);


// Trust Token issuer implementation.
//
// These functions implement the issuer half of the Trust Token protocol. A
// |TRUST_TOKEN_ISSUER| can be reused across multiple protocol operations. It
// may be used concurrently on multiple threads by non-mutating functions,
// provided no other thread is concurrently calling a mutating function.
// Functions which take a |const| pointer are non-mutating and functions which
// take a non-|const| pointer are mutating.

// TRUST_TOKEN_ISSUER_new returns a newly-allocated |TRUST_TOKEN_ISSUER|
// configured to use a max batchsize of |max_batchsize| or NULL on error.
// Issuance requests must be made in batches smaller than |max_batchsize|. This
// function will return an error if |max_batchsize| is too large for Trust
// Tokens.
OPENSSL_EXPORT TRUST_TOKEN_ISSUER *TRUST_TOKEN_ISSUER_new(
    const TRUST_TOKEN_METHOD *method, size_t max_batchsize);

// TRUST_TOKEN_ISSUER_free releases memory associated with |ctx|.
OPENSSL_EXPORT void TRUST_TOKEN_ISSUER_free(TRUST_TOKEN_ISSUER *ctx);

// TRUST_TOKEN_ISSUER_add_key configures the |ctx| to support the private key
// |key|. It must be a private key returned by |TRUST_TOKEN_generate_key|. It
// returns one on success or zero on error. This function may fail if the |key|
// can't be parsed or too many keys have been configured.
OPENSSL_EXPORT int TRUST_TOKEN_ISSUER_add_key(TRUST_TOKEN_ISSUER *ctx,
                                              const uint8_t *key,
                                              size_t key_len);

// TRUST_TOKEN_ISSUER_set_srr_key sets the private key used to sign the SRR. It
// returns one on success and zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_ISSUER_set_srr_key(TRUST_TOKEN_ISSUER *ctx,
                                                  EVP_PKEY *key);

// TRUST_TOKEN_ISSUER_set_metadata_key sets the key used to encrypt the private
// metadata. The key is a randomly generated bytestring of at least 32 bytes
// used to encode the private metadata bit in the SRR. It returns one on success
// and zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_ISSUER_set_metadata_key(TRUST_TOKEN_ISSUER *ctx,
                                                       const uint8_t *key,
                                                       size_t len);

// TRUST_TOKEN_ISSUER_issue ingests |request| for token issuance
// and generates up to |max_issuance| valid tokens, producing a list of blinded
// tokens and storing the response into a newly-allocated buffer and setting
// |*out| to that buffer, |*out_len| to its length, and |*out_tokens_issued| to
// the number of tokens issued. The tokens are issued with public metadata of
// |public_metadata| and a private metadata value of |private_metadata|.
// |public_metadata| must be one of the previously configured key IDs.
// |private_metadata| must be 0 or 1. The caller takes ownership of the buffer
// and must call |OPENSSL_free| when done. It returns one on success or zero on
// error.
OPENSSL_EXPORT int TRUST_TOKEN_ISSUER_issue(
    const TRUST_TOKEN_ISSUER *ctx, uint8_t **out, size_t *out_len,
    size_t *out_tokens_issued, const uint8_t *request, size_t request_len,
    uint32_t public_metadata, uint8_t private_metadata, size_t max_issuance);

// TRUST_TOKEN_ISSUER_redeem ingests a |request| for token redemption and
// verifies the token. The public metadata is stored in |*out_public|. The
// private metadata (if any) is stored in |*out_private|. The extracted
// |TRUST_TOKEN| is stored into a newly-allocated buffer and stored in
// |*out_token|. The extracted client data is stored into a newly-allocated
// buffer and stored in |*out_client_data|. The caller takes ownership of each
// output buffer and must call |OPENSSL_free| when done. It returns one on
// success or zero on error.
//
// The caller must keep track of all values of |*out_token| seen globally before
// returning a response to the client. If the value has been reused, the caller
// must report an error to the client. Returning a response with replayed values
// allows an attacker to double-spend tokens.
OPENSSL_EXPORT int TRUST_TOKEN_ISSUER_redeem(
    const TRUST_TOKEN_ISSUER *ctx, uint32_t *out_public, uint8_t *out_private,
    TRUST_TOKEN **out_token, uint8_t **out_client_data,
    size_t *out_client_data_len, const uint8_t *request, size_t request_len);

// TRUST_TOKEN_ISSUER_redeem_raw is a legacy alias for
// |TRUST_TOKEN_ISSUER_redeem|.
#define TRUST_TOKEN_ISSUER_redeem_raw TRUST_TOKEN_ISSUER_redeem

// TRUST_TOKEN_ISSUER_redeem_over_message ingests a |request| for token
// redemption and a message and verifies the token and that it is derived from
// the provided |msg|. The public metadata is stored in
// |*out_public|. The private metadata (if any) is stored in |*out_private|. The
// extracted |TRUST_TOKEN| is stored into a newly-allocated buffer and stored in
// |*out_token|. The extracted client data is stored into a newly-allocated
// buffer and stored in |*out_client_data|. The caller takes ownership of each
// output buffer and must call |OPENSSL_free| when done. It returns one on
// success or zero on error.
//
// The caller must keep track of all values of |*out_token| seen globally before
// returning a response to the client. If the value has been reused, the caller
// must report an error to the client. Returning a response with replayed values
// allows an attacker to double-spend tokens.
OPENSSL_EXPORT int TRUST_TOKEN_ISSUER_redeem_over_message(
    const TRUST_TOKEN_ISSUER *ctx, uint32_t *out_public, uint8_t *out_private,
    TRUST_TOKEN **out_token, uint8_t **out_client_data,
    size_t *out_client_data_len, const uint8_t *request, size_t request_len,
    const uint8_t *msg, size_t msg_len);

// TRUST_TOKEN_decode_private_metadata decodes |encrypted_bit| using the
// private metadata key specified by a |key| buffer of length |key_len| and the
// nonce by a |nonce| buffer of length |nonce_len|. The nonce in
// |TRUST_TOKEN_experiment_v1| is the token-hash field of the SRR. |*out_value|
// is set to the decrypted value, either zero or one. It returns one on success
// and zero on error.
OPENSSL_EXPORT int TRUST_TOKEN_decode_private_metadata(
    const TRUST_TOKEN_METHOD *method, uint8_t *out_value, const uint8_t *key,
    size_t key_len, const uint8_t *nonce, size_t nonce_len,
    uint8_t encrypted_bit);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(TRUST_TOKEN, TRUST_TOKEN_free)
BORINGSSL_MAKE_DELETER(TRUST_TOKEN_CLIENT, TRUST_TOKEN_CLIENT_free)
BORINGSSL_MAKE_DELETER(TRUST_TOKEN_ISSUER, TRUST_TOKEN_ISSUER_free)

BSSL_NAMESPACE_END

}  // extern C++
#endif

#define TRUST_TOKEN_R_KEYGEN_FAILURE 100
#define TRUST_TOKEN_R_BUFFER_TOO_SMALL 101
#define TRUST_TOKEN_R_OVER_BATCHSIZE 102
#define TRUST_TOKEN_R_DECODE_ERROR 103
#define TRUST_TOKEN_R_SRR_SIGNATURE_ERROR 104
#define TRUST_TOKEN_R_DECODE_FAILURE 105
#define TRUST_TOKEN_R_INVALID_METADATA 106
#define TRUST_TOKEN_R_TOO_MANY_KEYS 107
#define TRUST_TOKEN_R_NO_KEYS_CONFIGURED 108
#define TRUST_TOKEN_R_INVALID_KEY_ID 109
#define TRUST_TOKEN_R_INVALID_TOKEN 110
#define TRUST_TOKEN_R_BAD_VALIDITY_CHECK 111
#define TRUST_TOKEN_R_NO_SRR_KEY_CONFIGURED 112
#define TRUST_TOKEN_R_INVALID_METADATA_KEY 113
#define TRUST_TOKEN_R_INVALID_PROOF 114

#endif  // OPENSSL_HEADER_TRUST_TOKEN_H
