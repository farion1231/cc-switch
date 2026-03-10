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

#ifndef OPENSSL_HEADER_CURVE25519_H
#define OPENSSL_HEADER_CURVE25519_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Curve25519.
//
// Curve25519 is an elliptic curve. See https://tools.ietf.org/html/rfc7748.


// X25519.
//
// X25519 is the Diffie-Hellman primitive built from curve25519. It is
// sometimes referred to as “curve25519”, but “X25519” is a more precise name.
// See http://cr.yp.to/ecdh.html and https://tools.ietf.org/html/rfc7748.

#define X25519_PRIVATE_KEY_LEN 32
#define X25519_PUBLIC_VALUE_LEN 32
#define X25519_SHARED_KEY_LEN 32

// X25519_keypair sets |out_public_value| and |out_private_key| to a freshly
// generated, public–private key pair.
OPENSSL_EXPORT void X25519_keypair(
  uint8_t out_public_value[X25519_PUBLIC_VALUE_LEN],
  uint8_t out_private_key[X25519_PRIVATE_KEY_LEN]);

// X25519 writes a shared key to |out_shared_key| that is calculated from the
// given private key and the peer's public value. It returns one on success and
// zero on error.
//
// Don't use the shared key directly, rather use a KDF and also include the two
// public values as inputs.
OPENSSL_EXPORT int X25519(uint8_t out_shared_key[X25519_SHARED_KEY_LEN],
  const uint8_t private_key[X25519_PRIVATE_KEY_LEN],
  const uint8_t peer_public_value[X25519_PUBLIC_VALUE_LEN]);

// X25519_public_from_private calculates a Diffie-Hellman public value from the
// given private key and writes it to |out_public_value|.
OPENSSL_EXPORT void X25519_public_from_private(
  uint8_t out_public_value[X25519_PUBLIC_VALUE_LEN],
  const uint8_t private_key[X25519_PRIVATE_KEY_LEN]);


// Ed25519.
//
// Ed25519 is a signature scheme using a twisted-Edwards curve that is
// birationally equivalent to curve25519.
//
// Note that, unlike RFC 8032's formulation, our private key representation
// includes a public key suffix to make multiple key signing operations with the
// same key more efficient. The RFC 8032 private key is referred to in this
// implementation as the "seed" and is the first 32 bytes of our private key.

#define ED25519_PRIVATE_KEY_LEN 64
#define ED25519_PRIVATE_KEY_SEED_LEN 32
#define ED25519_PUBLIC_KEY_LEN 32
#define ED25519_SIGNATURE_LEN 64
#define ED25519_SEED_LEN 32

// ED25519_keypair sets |out_public_key| and |out_private_key| to a freshly
// generated, public–private key pair.
OPENSSL_EXPORT void ED25519_keypair(uint8_t out_public_key[ED25519_PUBLIC_KEY_LEN],
                                    uint8_t out_private_key[ED25519_PRIVATE_KEY_LEN]);

// ED25519_sign sets |out_sig| to be a signature of |message_len| bytes from
// |message| using |private_key|. It returns one on success or zero on
// allocation failure.
OPENSSL_EXPORT int ED25519_sign(uint8_t out_sig[ED25519_SIGNATURE_LEN],
                                const uint8_t *message, size_t message_len,
                                const uint8_t private_key[ED25519_PRIVATE_KEY_LEN]);

// ED25519_verify returns one iff |signature| is a valid signature, by
// |public_key| of |message_len| bytes from |message|. It returns zero
// otherwise.
OPENSSL_EXPORT int ED25519_verify(const uint8_t *message, size_t message_len,
                                  const uint8_t signature[ED25519_SIGNATURE_LEN],
                                  const uint8_t public_key[ED25519_PUBLIC_KEY_LEN]);

// ED25519ctx_sign sets |out_sig| to be a Ed25519ctx (RFC 8032) pure signature
// of |message_len| bytes from |message| using |private_key|, and the provided
// |context_len| bytes for |context|. |context_len| must be greater than zero,
// but no more than 255. It returns one on success or zero on failure.
OPENSSL_EXPORT int ED25519ctx_sign(
    uint8_t out_sig[ED25519_SIGNATURE_LEN],
    const uint8_t *message, size_t message_len,
    const uint8_t private_key[ED25519_PRIVATE_KEY_LEN],
    const uint8_t *context, size_t context_len);

// ED25519ctx_verify returns one iff |signature| is a valid Ed25519ctx pure
// signature, by |public_key| of |message_len| bytes from |message|, and
// |context_len| bytes from |context|. |context_len| must be greater than zero,
// but no more than 255. It returns zero otherwise.
OPENSSL_EXPORT int ED25519ctx_verify(const uint8_t *message, size_t message_len,
                                     const uint8_t signature[ED25519_SIGNATURE_LEN],
                                     const uint8_t public_key[ED25519_PUBLIC_KEY_LEN],
                                     const uint8_t *context, size_t context_len);

// ED25519ph_sign sets |out_sig| to be a Ed25519ph (RFC 8032) / HashEdDSA
// signature of |message_len| bytes from |message| using |private_key|, and the
// provided |context_len| bytes for |context|. |context_len| may be zero, but no
// more than 255. It returns one on success or zero on failure.
OPENSSL_EXPORT int ED25519ph_sign(
    uint8_t out_sig[ED25519_SIGNATURE_LEN],
    const uint8_t *message, size_t message_len,
    const uint8_t private_key[ED25519_PRIVATE_KEY_LEN],
    const uint8_t *context, size_t context_len);

// ED25519ph_verify returns one iff |signature| is a valid Ed25519ph (RFC 8032)
// / HashEdDSA signature, by |public_key| of |message_len| bytes from |message|,
// and |context_len| bytes from |context|. |context_len| may be zero, but no
// more than 255. It returns zero otherwise.
OPENSSL_EXPORT int ED25519ph_verify(const uint8_t *message, size_t message_len,
                                    const uint8_t signature[ED25519_SIGNATURE_LEN],
                                    const uint8_t public_key[ED25519_PUBLIC_KEY_LEN],
                                    const uint8_t *context, size_t context_len);

// ED25519ph_sign_digest sets |out_sig| to be a Ed25519ph (RFC 8032) / HashEdDSA
// signature of a pre-computed SHA-512 message digest |digest| using
// |private_key|, and the provided |context_len| bytes for |context|.
// |context_len| may be zero, but no more than 255.
// It returns one on success or zero on failure.
OPENSSL_EXPORT int ED25519ph_sign_digest(
    uint8_t out_sig[ED25519_SIGNATURE_LEN],
    const uint8_t digest[64],
    const uint8_t private_key[ED25519_PRIVATE_KEY_LEN],
    const uint8_t *context, size_t context_len);

// ED25519ph_verify_digest returns one iff |signature| is a valid Ed25519ph (RFC
// 8032) / HashEdDSA signature, by |public_key| of a pre-computed SHA-512
// message digest |digest|, and |context_len| bytes from |context|.
// |context_len| may be zero, but no more than 255.
// It returns zero otherwise.
OPENSSL_EXPORT int ED25519ph_verify_digest(const uint8_t digest[64],
                                           const uint8_t signature[ED25519_SIGNATURE_LEN],
                                           const uint8_t public_key[ED25519_PUBLIC_KEY_LEN],
                                           const uint8_t *context, size_t context_len);

// ED25519_keypair_from_seed calculates a public and private key from an
// Ed25519 “seed”. Seed values are not exposed by this API (although they
// happen to be the first 32 bytes of a private key) so this function is for
// interoperating with systems that may store just a seed instead of a full
// private key.
OPENSSL_EXPORT void ED25519_keypair_from_seed(uint8_t out_public_key[ED25519_PUBLIC_KEY_LEN],
                                              uint8_t out_private_key[ED25519_PRIVATE_KEY_LEN],
                                              const uint8_t seed[ED25519_SEED_LEN]);


// SPAKE2.
//
// SPAKE2 is a password-authenticated key-exchange. It allows two parties,
// who share a low-entropy secret (i.e. password), to agree on a shared key.
// An attacker can only make one guess of the password per execution of the
// protocol.
//
// See https://tools.ietf.org/html/draft-irtf-cfrg-spake2-02.

// spake2_role_t enumerates the different “roles” in SPAKE2. The protocol
// requires that the symmetry of the two parties be broken so one participant
// must be “Alice” and the other be “Bob”.
enum spake2_role_t {
  spake2_role_alice,
  spake2_role_bob
};

// SPAKE2_CTX_new creates a new |SPAKE2_CTX| (which can only be used for a
// single execution of the protocol). SPAKE2 requires the symmetry of the two
// parties to be broken which is indicated via |my_role| – each party must pass
// a different value for this argument.
//
// The |my_name| and |their_name| arguments allow optional, opaque names to be
// bound into the protocol. For example MAC addresses, hostnames, usernames
// etc. These values are not exposed and can avoid context-confusion attacks
// when a password is shared between several devices.
OPENSSL_EXPORT SPAKE2_CTX *SPAKE2_CTX_new(
    enum spake2_role_t my_role,
    const uint8_t *my_name, size_t my_name_len,
    const uint8_t *their_name, size_t their_name_len);

// SPAKE2_CTX_free frees |ctx| and all the resources that it has allocated.
OPENSSL_EXPORT void SPAKE2_CTX_free(SPAKE2_CTX *ctx);

// SPAKE2_MAX_MSG_SIZE is the maximum size of a SPAKE2 message.
#define SPAKE2_MAX_MSG_SIZE 32

// SPAKE2_generate_msg generates a SPAKE2 message given |password|, writes
// it to |out| and sets |*out_len| to the number of bytes written.
//
// At most |max_out_len| bytes are written to |out| and, in order to ensure
// success, |max_out_len| should be at least |SPAKE2_MAX_MSG_SIZE| bytes.
//
// This function can only be called once for a given |SPAKE2_CTX|.
//
// It returns one on success and zero on error.
OPENSSL_EXPORT int SPAKE2_generate_msg(SPAKE2_CTX *ctx, uint8_t *out,
                                       size_t *out_len, size_t max_out_len,
                                       const uint8_t *password,
                                       size_t password_len);

// SPAKE2_MAX_KEY_SIZE is the maximum amount of key material that SPAKE2 will
// produce.
#define SPAKE2_MAX_KEY_SIZE 64

// SPAKE2_process_msg completes the SPAKE2 exchange given the peer's message in
// |their_msg|, writes at most |max_out_key_len| bytes to |out_key| and sets
// |*out_key_len| to the number of bytes written.
//
// The resulting keying material is suitable for:
//    - Using directly in a key-confirmation step: i.e. each side could
//      transmit a hash of their role, a channel-binding value and the key
//      material to prove to the other side that they know the shared key.
//   -  Using as input keying material to HKDF to generate a variety of subkeys
//      for encryption etc.
//
// If |max_out_key_key| is smaller than the amount of key material generated
// then the key is silently truncated. If you want to ensure that no truncation
// occurs then |max_out_key| should be at least |SPAKE2_MAX_KEY_SIZE|.
//
// You must call |SPAKE2_generate_msg| on a given |SPAKE2_CTX| before calling
// this function. On successful return, |ctx| is complete and calling
// |SPAKE2_CTX_free| is the only acceptable operation on it.
//
// Returns one on success or zero on error.
OPENSSL_EXPORT int SPAKE2_process_msg(SPAKE2_CTX *ctx, uint8_t *out_key,
                                      size_t *out_key_len,
                                      size_t max_out_key_len,
                                      const uint8_t *their_msg,
                                      size_t their_msg_len);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(SPAKE2_CTX, SPAKE2_CTX_free)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#endif  // OPENSSL_HEADER_CURVE25519_H
