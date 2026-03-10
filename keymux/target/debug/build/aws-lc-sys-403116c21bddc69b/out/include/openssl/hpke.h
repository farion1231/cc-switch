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

#ifndef OPENSSL_HEADER_CRYPTO_HPKE_INTERNAL_H
#define OPENSSL_HEADER_CRYPTO_HPKE_INTERNAL_H

#include <openssl/aead.h>
#include <openssl/base.h>
#include <openssl/curve25519.h>
#include <openssl/digest.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Hybrid Public Key Encryption.
//
// Hybrid Public Key Encryption (HPKE) enables a sender to encrypt messages to a
// receiver with a public key.
//
// See RFC 9180.


// Parameters.
//
// An HPKE context is parameterized by KEM, KDF, and AEAD algorithms,
// represented by |EVP_HPKE_KEM|, |EVP_HPKE_KDF|, and |EVP_HPKE_AEAD| types,
// respectively.

// The following constants are KEM identifiers.
#define EVP_HPKE_DHKEM_X25519_HKDF_SHA256 0x0020

// The following functions are KEM algorithms which may be used with HPKE. Note
// that, while some HPKE KEMs use KDFs internally, this is separate from the
// |EVP_HPKE_KDF| selection.
OPENSSL_EXPORT const EVP_HPKE_KEM *EVP_hpke_x25519_hkdf_sha256(void);

// EVP_HPKE_KEM_id returns the HPKE KEM identifier for |kem|, which
// will be one of the |EVP_HPKE_KEM_*| constants.
OPENSSL_EXPORT uint16_t EVP_HPKE_KEM_id(const EVP_HPKE_KEM *kem);

// EVP_HPKE_MAX_PUBLIC_KEY_LENGTH is the maximum length of an encoded public key
// for all KEMs currently supported by this library.
#define EVP_HPKE_MAX_PUBLIC_KEY_LENGTH 32

// EVP_HPKE_KEM_public_key_len returns the length of a public key for |kem|.
// This value will be at most |EVP_HPKE_MAX_PUBLIC_KEY_LENGTH|.
OPENSSL_EXPORT size_t EVP_HPKE_KEM_public_key_len(const EVP_HPKE_KEM *kem);

// EVP_HPKE_MAX_PRIVATE_KEY_LENGTH is the maximum length of an encoded private
// key for all KEMs currently supported by this library.
#define EVP_HPKE_MAX_PRIVATE_KEY_LENGTH 32

// EVP_HPKE_KEM_private_key_len returns the length of a private key for |kem|.
// This value will be at most |EVP_HPKE_MAX_PRIVATE_KEY_LENGTH|.
OPENSSL_EXPORT size_t EVP_HPKE_KEM_private_key_len(const EVP_HPKE_KEM *kem);

// EVP_HPKE_MAX_ENC_LENGTH is the maximum length of "enc", the encapsulated
// shared secret, for all KEMs currently supported by this library.
#define EVP_HPKE_MAX_ENC_LENGTH 32

// EVP_HPKE_KEM_enc_len returns the length of the "enc", the encapsulated shared
// secret, for |kem|. This value will be at most |EVP_HPKE_MAX_ENC_LENGTH|.
OPENSSL_EXPORT size_t EVP_HPKE_KEM_enc_len(const EVP_HPKE_KEM *kem);

// The following constants are KDF identifiers.
#define EVP_HPKE_HKDF_SHA256 0x0001

// The following functions are KDF algorithms which may be used with HPKE.
OPENSSL_EXPORT const EVP_HPKE_KDF *EVP_hpke_hkdf_sha256(void);

// EVP_HPKE_KDF_id returns the HPKE KDF identifier for |kdf|.
OPENSSL_EXPORT uint16_t EVP_HPKE_KDF_id(const EVP_HPKE_KDF *kdf);

// EVP_HPKE_KDF_hkdf_md returns the HKDF hash function corresponding to |kdf|,
// or NULL if |kdf| is not an HKDF-based KDF. All currently supported KDFs are
// HKDF-based.
OPENSSL_EXPORT const EVP_MD *EVP_HPKE_KDF_hkdf_md(const EVP_HPKE_KDF *kdf);

// The following constants are AEAD identifiers.
#define EVP_HPKE_AES_128_GCM 0x0001
#define EVP_HPKE_AES_256_GCM 0x0002
#define EVP_HPKE_CHACHA20_POLY1305 0x0003

// The following functions are AEAD algorithms which may be used with HPKE.
OPENSSL_EXPORT const EVP_HPKE_AEAD *EVP_hpke_aes_128_gcm(void);
OPENSSL_EXPORT const EVP_HPKE_AEAD *EVP_hpke_aes_256_gcm(void);
OPENSSL_EXPORT const EVP_HPKE_AEAD *EVP_hpke_chacha20_poly1305(void);

// EVP_HPKE_AEAD_id returns the HPKE AEAD identifier for |aead|.
OPENSSL_EXPORT uint16_t EVP_HPKE_AEAD_id(const EVP_HPKE_AEAD *aead);

// EVP_HPKE_AEAD_aead returns the |EVP_AEAD| corresponding to |aead|.
OPENSSL_EXPORT const EVP_AEAD *EVP_HPKE_AEAD_aead(const EVP_HPKE_AEAD *aead);


// Recipient keys.
//
// An HPKE recipient maintains a long-term KEM key. This library represents keys
// with the |EVP_HPKE_KEY| type.

// EVP_HPKE_KEY_zero sets an uninitialized |EVP_HPKE_KEY| to the zero state. The
// caller should then use |EVP_HPKE_KEY_init|, |EVP_HPKE_KEY_copy|, or
// |EVP_HPKE_KEY_generate| to finish initializing |key|.
//
// It is safe, but not necessary to call |EVP_HPKE_KEY_cleanup| in this state.
// This may be used for more uniform cleanup of |EVP_HPKE_KEY|.
OPENSSL_EXPORT void EVP_HPKE_KEY_zero(EVP_HPKE_KEY *key);

// EVP_HPKE_KEY_cleanup releases memory referenced by |key|.
OPENSSL_EXPORT void EVP_HPKE_KEY_cleanup(EVP_HPKE_KEY *key);

// EVP_HPKE_KEY_new returns a newly-allocated |EVP_HPKE_KEY|, or NULL on error.
// The caller must call |EVP_HPKE_KEY_free| on the result to release it.
//
// This is a convenience function for callers that need a heap-allocated
// |EVP_HPKE_KEY|.
OPENSSL_EXPORT EVP_HPKE_KEY *EVP_HPKE_KEY_new(void);

// EVP_HPKE_KEY_free releases memory associated with |key|, which must have been
// created with |EVP_HPKE_KEY_new|.
OPENSSL_EXPORT void EVP_HPKE_KEY_free(EVP_HPKE_KEY *key);

// EVP_HPKE_KEY_copy sets |dst| to a copy of |src|. It returns one on success
// and zero on error. On success, the caller must call |EVP_HPKE_KEY_cleanup| to
// release |dst|. On failure, calling |EVP_HPKE_KEY_cleanup| is safe, but not
// necessary.
OPENSSL_EXPORT int EVP_HPKE_KEY_copy(EVP_HPKE_KEY *dst,
                                     const EVP_HPKE_KEY *src);

// EVP_HPKE_KEY_move sets |out|, which must be initialized or in the zero state,
// to the key in |in|. |in| is mutated and left in the zero state.
OPENSSL_EXPORT void EVP_HPKE_KEY_move(EVP_HPKE_KEY *out, EVP_HPKE_KEY *in);

// EVP_HPKE_KEY_init decodes |priv_key| as a private key for |kem| and
// initializes |key| with the result. It returns one on success and zero if
// |priv_key| was invalid. On success, the caller must call
// |EVP_HPKE_KEY_cleanup| to release the key. On failure, calling
// |EVP_HPKE_KEY_cleanup| is safe, but not necessary.
OPENSSL_EXPORT int EVP_HPKE_KEY_init(EVP_HPKE_KEY *key, const EVP_HPKE_KEM *kem,
                                     const uint8_t *priv_key,
                                     size_t priv_key_len);

// EVP_HPKE_KEY_generate sets |key| to a newly-generated key using |kem|.
OPENSSL_EXPORT int EVP_HPKE_KEY_generate(EVP_HPKE_KEY *key,
                                         const EVP_HPKE_KEM *kem);

// EVP_HPKE_KEY_kem returns the HPKE KEM used by |key|.
OPENSSL_EXPORT const EVP_HPKE_KEM *EVP_HPKE_KEY_kem(const EVP_HPKE_KEY *key);

// EVP_HPKE_KEY_public_key writes |key|'s public key to |out| and sets
// |*out_len| to the number of bytes written. On success, it returns one and
// writes at most |max_out| bytes. If |max_out| is too small, it returns zero.
// Setting |max_out| to |EVP_HPKE_MAX_PUBLIC_KEY_LENGTH| will ensure the public
// key fits. An exact size can also be determined by
// |EVP_HPKE_KEM_public_key_len|.
OPENSSL_EXPORT int EVP_HPKE_KEY_public_key(const EVP_HPKE_KEY *key,
                                           uint8_t *out, size_t *out_len,
                                           size_t max_out);

// EVP_HPKE_KEY_private_key writes |key|'s private key to |out| and sets
// |*out_len| to the number of bytes written. On success, it returns one and
// writes at most |max_out| bytes. If |max_out| is too small, it returns zero.
// Setting |max_out| to |EVP_HPKE_MAX_PRIVATE_KEY_LENGTH| will ensure the
// private key fits. An exact size can also be determined by
// |EVP_HPKE_KEM_private_key_len|.
OPENSSL_EXPORT int EVP_HPKE_KEY_private_key(const EVP_HPKE_KEY *key,
                                            uint8_t *out, size_t *out_len,
                                            size_t max_out);


// Encryption contexts.
//
// An HPKE encryption context is represented by the |EVP_HPKE_CTX| type.

// EVP_HPKE_CTX_zero sets an uninitialized |EVP_HPKE_CTX| to the zero state. The
// caller should then use one of the |EVP_HPKE_CTX_setup_*| functions to finish
// setting up |ctx|.
//
// It is safe, but not necessary to call |EVP_HPKE_CTX_cleanup| in this state.
// This may be used for more uniform cleanup of |EVP_HPKE_CTX|.
OPENSSL_EXPORT void EVP_HPKE_CTX_zero(EVP_HPKE_CTX *ctx);

// EVP_HPKE_CTX_cleanup releases memory referenced by |ctx|. |ctx| must have
// been initialized with |EVP_HPKE_CTX_zero| or one of the
// |EVP_HPKE_CTX_setup_*| functions.
OPENSSL_EXPORT void EVP_HPKE_CTX_cleanup(EVP_HPKE_CTX *ctx);

// EVP_HPKE_CTX_new returns a newly-allocated |EVP_HPKE_CTX|, or NULL on error.
// The caller must call |EVP_HPKE_CTX_free| on the result to release it.
//
// This is a convenience function for callers that need a heap-allocated
// |EVP_HPKE_CTX|.
OPENSSL_EXPORT EVP_HPKE_CTX *EVP_HPKE_CTX_new(void);

// EVP_HPKE_CTX_free releases memory associated with |ctx|, which must have been
// created with |EVP_HPKE_CTX_new|.
OPENSSL_EXPORT void EVP_HPKE_CTX_free(EVP_HPKE_CTX *ctx);

// EVP_HPKE_CTX_setup_sender implements the SetupBaseS HPKE operation. It
// encapsulates a shared secret for |peer_public_key| and sets up |ctx| as a
// sender context. It writes the encapsulated shared secret to |out_enc| and
// sets |*out_enc_len| to the number of bytes written. It writes at most
// |max_enc| bytes and fails if the buffer is too small. Setting |max_enc| to at
// least |EVP_HPKE_MAX_ENC_LENGTH| will ensure the buffer is large enough. An
// exact size may also be determined by |EVP_PKEY_KEM_enc_len|.
//
// This function returns one on success and zero on error. Note that
// |peer_public_key| may be invalid, in which case this function will return an
// error.
//
// On success, callers may call |EVP_HPKE_CTX_seal| to encrypt messages for the
// recipient. Callers must then call |EVP_HPKE_CTX_cleanup| when done. On
// failure, calling |EVP_HPKE_CTX_cleanup| is safe, but not required.
OPENSSL_EXPORT int EVP_HPKE_CTX_setup_sender(
    EVP_HPKE_CTX *ctx, uint8_t *out_enc, size_t *out_enc_len, size_t max_enc,
    const EVP_HPKE_KEM *kem, const EVP_HPKE_KDF *kdf, const EVP_HPKE_AEAD *aead,
    const uint8_t *peer_public_key, size_t peer_public_key_len,
    const uint8_t *info, size_t info_len);

// EVP_HPKE_CTX_setup_sender_with_seed_for_testing behaves like
// |EVP_HPKE_CTX_setup_sender|, but takes a seed to behave deterministically.
// The seed's format depends on |kem|. For X25519, it is the sender's
// ephemeral private key.
OPENSSL_EXPORT int EVP_HPKE_CTX_setup_sender_with_seed_for_testing(
    EVP_HPKE_CTX *ctx, uint8_t *out_enc, size_t *out_enc_len, size_t max_enc,
    const EVP_HPKE_KEM *kem, const EVP_HPKE_KDF *kdf, const EVP_HPKE_AEAD *aead,
    const uint8_t *peer_public_key, size_t peer_public_key_len,
    const uint8_t *info, size_t info_len, const uint8_t *seed, size_t seed_len);

// EVP_HPKE_CTX_setup_recipient implements the SetupBaseR HPKE operation. It
// decapsulates the shared secret in |enc| with |key| and sets up |ctx| as a
// recipient context. It returns one on success and zero on failure. Note that
// |enc| may be invalid, in which case this function will return an error.
//
// On success, callers may call |EVP_HPKE_CTX_open| to decrypt messages from the
// sender. Callers must then call |EVP_HPKE_CTX_cleanup| when done. On failure,
// calling |EVP_HPKE_CTX_cleanup| is safe, but not required.
OPENSSL_EXPORT int EVP_HPKE_CTX_setup_recipient(
    EVP_HPKE_CTX *ctx, const EVP_HPKE_KEY *key, const EVP_HPKE_KDF *kdf,
    const EVP_HPKE_AEAD *aead, const uint8_t *enc, size_t enc_len,
    const uint8_t *info, size_t info_len);

// EVP_HPKE_CTX_setup_auth_sender implements the SetupAuthS HPKE operation. It
// behaves like |EVP_HPKE_CTX_setup_sender| but authenticates the resulting
// context with |key|.
OPENSSL_EXPORT int EVP_HPKE_CTX_setup_auth_sender(
    EVP_HPKE_CTX *ctx, uint8_t *out_enc, size_t *out_enc_len, size_t max_enc,
    const EVP_HPKE_KEY *key, const EVP_HPKE_KDF *kdf, const EVP_HPKE_AEAD *aead,
    const uint8_t *peer_public_key, size_t peer_public_key_len,
    const uint8_t *info, size_t info_len);

// EVP_HPKE_CTX_setup_auth_sender_with_seed_for_testing behaves like
// |EVP_HPKE_CTX_setup_auth_sender|, but takes a seed to behave
// deterministically. The seed's format depends on |kem|. For X25519, it is the
// sender's ephemeral private key.
OPENSSL_EXPORT int EVP_HPKE_CTX_setup_auth_sender_with_seed_for_testing(
    EVP_HPKE_CTX *ctx, uint8_t *out_enc, size_t *out_enc_len, size_t max_enc,
    const EVP_HPKE_KEY *key, const EVP_HPKE_KDF *kdf, const EVP_HPKE_AEAD *aead,
    const uint8_t *peer_public_key, size_t peer_public_key_len,
    const uint8_t *info, size_t info_len, const uint8_t *seed, size_t seed_len);

// EVP_HPKE_CTX_setup_auth_recipient implements the SetupAuthR HPKE operation.
// It behaves like |EVP_HPKE_CTX_setup_recipient| but checks the resulting
// context was authenticated with |peer_public_key|.
OPENSSL_EXPORT int EVP_HPKE_CTX_setup_auth_recipient(
    EVP_HPKE_CTX *ctx, const EVP_HPKE_KEY *key, const EVP_HPKE_KDF *kdf,
    const EVP_HPKE_AEAD *aead, const uint8_t *enc, size_t enc_len,
    const uint8_t *info, size_t info_len, const uint8_t *peer_public_key,
    size_t peer_public_key_len);


// Using an HPKE context.
//
// Once set up, callers may encrypt or decrypt with an |EVP_HPKE_CTX| using the
// following functions.

// EVP_HPKE_CTX_open uses the HPKE context |ctx| to authenticate |in_len| bytes
// from |in| and |ad_len| bytes from |ad| and to decrypt at most |in_len| bytes
// into |out|. It returns one on success, and zero otherwise.
//
// This operation will fail if the |ctx| context is not set up as a receiver.
//
// Note that HPKE encryption is stateful and ordered. The sender's first call to
// |EVP_HPKE_CTX_seal| must correspond to the recipient's first call to
// |EVP_HPKE_CTX_open|, etc.
//
// At most |in_len| bytes are written to |out|. In order to ensure success,
// |max_out_len| should be at least |in_len|. On successful return, |*out_len|
// is set to the actual number of bytes written.
OPENSSL_EXPORT int EVP_HPKE_CTX_open(EVP_HPKE_CTX *ctx, uint8_t *out,
                                     size_t *out_len, size_t max_out_len,
                                     const uint8_t *in, size_t in_len,
                                     const uint8_t *ad, size_t ad_len);

// EVP_HPKE_CTX_seal uses the HPKE context |ctx| to encrypt and authenticate
// |in_len| bytes of ciphertext |in| and authenticate |ad_len| bytes from |ad|,
// writing the result to |out|. It returns one on success and zero otherwise.
//
// This operation will fail if the |ctx| context is not set up as a sender.
//
// Note that HPKE encryption is stateful and ordered. The sender's first call to
// |EVP_HPKE_CTX_seal| must correspond to the recipient's first call to
// |EVP_HPKE_CTX_open|, etc.
//
// At most, |max_out_len| encrypted bytes are written to |out|. On successful
// return, |*out_len| is set to the actual number of bytes written.
//
// To ensure success, |max_out_len| should be |in_len| plus the result of
// |EVP_HPKE_CTX_max_overhead| or |EVP_HPKE_MAX_OVERHEAD|.
OPENSSL_EXPORT int EVP_HPKE_CTX_seal(EVP_HPKE_CTX *ctx, uint8_t *out,
                                     size_t *out_len, size_t max_out_len,
                                     const uint8_t *in, size_t in_len,
                                     const uint8_t *ad, size_t ad_len);

// EVP_HPKE_CTX_export uses the HPKE context |ctx| to export a secret of
// |secret_len| bytes into |out|. This function uses |context_len| bytes from
// |context| as a context string for the secret. This is necessary to separate
// different uses of exported secrets and bind relevant caller-specific context
// into the output. It returns one on success and zero otherwise.
OPENSSL_EXPORT int EVP_HPKE_CTX_export(const EVP_HPKE_CTX *ctx, uint8_t *out,
                                       size_t secret_len,
                                       const uint8_t *context,
                                       size_t context_len);

// EVP_HPKE_MAX_OVERHEAD contains the largest value that
// |EVP_HPKE_CTX_max_overhead| would ever return for any context.
#define EVP_HPKE_MAX_OVERHEAD EVP_AEAD_MAX_OVERHEAD

// EVP_HPKE_CTX_max_overhead returns the maximum number of additional bytes
// added by sealing data with |EVP_HPKE_CTX_seal|. The |ctx| context must be set
// up as a sender.
OPENSSL_EXPORT size_t EVP_HPKE_CTX_max_overhead(const EVP_HPKE_CTX *ctx);

// EVP_HPKE_CTX_kem returns |ctx|'s configured KEM, or NULL if the context has
// not been set up.
OPENSSL_EXPORT const EVP_HPKE_KEM *EVP_HPKE_CTX_kem(const EVP_HPKE_CTX *ctx);

// EVP_HPKE_CTX_aead returns |ctx|'s configured AEAD, or NULL if the context has
// not been set up.
OPENSSL_EXPORT const EVP_HPKE_AEAD *EVP_HPKE_CTX_aead(const EVP_HPKE_CTX *ctx);

// EVP_HPKE_CTX_kdf returns |ctx|'s configured KDF, or NULL if the context has
// not been set up.
OPENSSL_EXPORT const EVP_HPKE_KDF *EVP_HPKE_CTX_kdf(const EVP_HPKE_CTX *ctx);


// Private structures.
//
// The following structures are exported so their types are stack-allocatable,
// but accessing or modifying their fields is forbidden.

struct evp_hpke_ctx_st {
  const EVP_HPKE_KEM *kem;
  const EVP_HPKE_AEAD *aead;
  const EVP_HPKE_KDF *kdf;
  EVP_AEAD_CTX aead_ctx;
  uint8_t base_nonce[EVP_AEAD_MAX_NONCE_LENGTH];
  uint8_t exporter_secret[EVP_MAX_MD_SIZE];
  uint64_t seq;
  int is_sender;
};

struct evp_hpke_key_st {
  const EVP_HPKE_KEM *kem;
  uint8_t private_key[X25519_PRIVATE_KEY_LEN];
  uint8_t public_key[X25519_PUBLIC_VALUE_LEN];
};


#if defined(__cplusplus)
}  // extern C
#endif

#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

using ScopedEVP_HPKE_CTX =
    internal::StackAllocated<EVP_HPKE_CTX, void, EVP_HPKE_CTX_zero,
                             EVP_HPKE_CTX_cleanup>;
using ScopedEVP_HPKE_KEY =
    internal::StackAllocatedMovable<EVP_HPKE_KEY, void, EVP_HPKE_KEY_zero,
                                    EVP_HPKE_KEY_cleanup, EVP_HPKE_KEY_move>;

BORINGSSL_MAKE_DELETER(EVP_HPKE_CTX, EVP_HPKE_CTX_free)
BORINGSSL_MAKE_DELETER(EVP_HPKE_KEY, EVP_HPKE_KEY_free)

BSSL_NAMESPACE_END

}  // extern C++
#endif

#endif  // OPENSSL_HEADER_CRYPTO_HPKE_INTERNAL_H
