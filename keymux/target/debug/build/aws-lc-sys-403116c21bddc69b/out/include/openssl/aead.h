/* Copyright (c) 2014, Google Inc.
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

#ifndef OPENSSL_HEADER_AEAD_H
#define OPENSSL_HEADER_AEAD_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Authenticated Encryption with Additional Data.
//
// AEAD couples confidentiality and integrity in a single primitive. AEAD
// algorithms take a key and then can seal and open individual messages. Each
// message has a unique, per-message nonce and, optionally, additional data
// which is authenticated but not included in the ciphertext.
//
// The |EVP_AEAD_CTX_init| function initialises an |EVP_AEAD_CTX| structure and
// performs any precomputation needed to use |aead| with |key|. The length of
// the key, |key_len|, is given in bytes.
//
// The |tag_len| argument contains the length of the tags, in bytes, and allows
// for the processing of truncated authenticators. A zero value indicates that
// the default tag length should be used and this is defined as
// |EVP_AEAD_DEFAULT_TAG_LENGTH| in order to make the code clear. Using
// truncated tags increases an attacker's chance of creating a valid forgery.
// Be aware that the attacker's chance may increase more than exponentially as
// would naively be expected.
//
// When no longer needed, the initialised |EVP_AEAD_CTX| structure must be
// passed to |EVP_AEAD_CTX_cleanup|, which will deallocate any memory used.
//
// With an |EVP_AEAD_CTX| in hand, one can seal and open messages. These
// operations are intended to meet the standard notions of privacy and
// authenticity for authenticated encryption. For formal definitions see
// Bellare and Namprempre, "Authenticated encryption: relations among notions
// and analysis of the generic composition paradigm," Lecture Notes in Computer
// Science B<1976> (2000), 531–545,
// http://www-cse.ucsd.edu/~mihir/papers/oem.html.
//
// When sealing messages, a nonce must be given. The length of the nonce is
// fixed by the AEAD in use and is returned by |EVP_AEAD_nonce_length|. *The
// nonce must be unique for all messages with the same key*. This is critically
// important - nonce reuse may completely undermine the security of the AEAD.
// Nonces may be predictable and public, so long as they are unique. Uniqueness
// may be achieved with a simple counter or, if large enough, may be generated
// randomly. The nonce must be passed into the "open" operation by the receiver
// so must either be implicit (e.g. a counter), or must be transmitted along
// with the sealed message.
//
// The "seal" and "open" operations are atomic - an entire message must be
// encrypted or decrypted in a single call. Large messages may have to be split
// up in order to accommodate this. When doing so, be mindful of the need not to
// repeat nonces and the possibility that an attacker could duplicate, reorder
// or drop message chunks. For example, using a single key for a given (large)
// message and sealing chunks with nonces counting from zero would be secure as
// long as the number of chunks was securely transmitted. (Otherwise an
// attacker could truncate the message by dropping chunks from the end.)
//
// The number of chunks could be transmitted by prefixing it to the plaintext,
// for example. This also assumes that no other message would ever use the same
// key otherwise the rule that nonces must be unique for a given key would be
// violated.
//
// The "seal" and "open" operations also permit additional data to be
// authenticated via the |ad| parameter. This data is not included in the
// ciphertext and must be identical for both the "seal" and "open" call. This
// permits implicit context to be authenticated but may be empty if not needed.
//
// The "seal" and "open" operations may work in-place if the |out| and |in|
// arguments are equal. Otherwise, if |out| and |in| alias, input data may be
// overwritten before it is read. This situation will cause an error.
//
// The "seal" and "open" operations return one on success and zero on error.


// AEAD algorithms.

// EVP_aead_aes_128_gcm is AES-128 in Galois Counter Mode.
//
// Note: AES-GCM should only be used with 12-byte (96-bit) nonces. Although it
// is specified to take a variable-length nonce, nonces with other lengths are
// effectively randomized, which means one must consider collisions. Unless
// implementing an existing protocol which has already specified incorrect
// parameters, only use 12-byte nonces.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_gcm(void);

// EVP_aead_aes_192_gcm is AES-192 in Galois Counter Mode.
//
// WARNING: AES-192 is superfluous and shouldn't exist. NIST should never have
// defined it. Use only when interop with another system requires it, never
// de novo.
//
// Note: AES-GCM should only be used with 12-byte (96-bit) nonces. Although it
// is specified to take a variable-length nonce, nonces with other lengths are
// effectively randomized, which means one must consider collisions. Unless
// implementing an existing protocol which has already specified incorrect
// parameters, only use 12-byte nonces.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_192_gcm(void);

// EVP_aead_aes_256_gcm is AES-256 in Galois Counter Mode.
//
// Note: AES-GCM should only be used with 12-byte (96-bit) nonces. Although it
// is specified to take a variable-length nonce, nonces with other lengths are
// effectively randomized, which means one must consider collisions. Unless
// implementing an existing protocol which has already specified incorrect
// parameters, only use 12-byte nonces.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_gcm(void);

// EVP_aead_chacha20_poly1305 is the AEAD built from ChaCha20 and
// Poly1305 as described in RFC 8439.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_chacha20_poly1305(void);

// EVP_aead_xchacha20_poly1305 is ChaCha20-Poly1305 with an extended nonce that
// makes random generation of nonces safe.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_xchacha20_poly1305(void);

// EVP_aead_aes_128_ctr_hmac_sha256 is AES-128 in CTR mode with HMAC-SHA256 for
// authentication. The nonce is 12 bytes; the bottom 32-bits are used as the
// block counter, thus the maximum plaintext size is 64GB.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_ctr_hmac_sha256(void);

// EVP_aead_aes_256_ctr_hmac_sha256 is AES-256 in CTR mode with HMAC-SHA256 for
// authentication. See |EVP_aead_aes_128_ctr_hmac_sha256| for details.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_ctr_hmac_sha256(void);

// EVP_aead_aes_128_gcm_siv is AES-128 in GCM-SIV mode. See RFC 8452.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_gcm_siv(void);

// EVP_aead_aes_256_gcm_siv is AES-256 in GCM-SIV mode. See RFC 8452.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_gcm_siv(void);

// EVP_aead_aes_128_gcm_randnonce is AES-128 in Galois Counter Mode with
// internal nonce generation. The 12-byte nonce is appended to the tag
// and is generated internally. The "tag", for the purposes of the API, is thus
// 12 bytes larger. The nonce parameter when using this AEAD must be
// zero-length. Since the nonce is random, a single key should not be used for
// more than 2^32 seal operations.
//
// Warning: this is for use for FIPS compliance only. It is probably not
// suitable for other uses. Using standard AES-GCM AEADs allows one to achieve
// the same effect, but gives more control over nonce storage.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_gcm_randnonce(void);

// EVP_aead_aes_256_gcm_randnonce is AES-256 in Galois Counter Mode with
// internal nonce generation. The 12-byte nonce is appended to the tag
// and is generated internally. The "tag", for the purposes of the API, is thus
// 12 bytes larger. The nonce parameter when using this AEAD must be
// zero-length. Since the nonce is random, a single key should not be used for
// more than 2^32 seal operations.
//
// Warning: this is for use for FIPS compliance only. It is probably not
// suitable for other uses. Using standard AES-GCM AEADs allows one to achieve
// the same effect, but gives more control over nonce storage.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_gcm_randnonce(void);

// EVP_aead_aes_128_ccm_bluetooth is AES-128-CCM with M=4 and L=2 (4-byte tags
// and 13-byte nonces), as described in the Bluetooth Core Specification v5.0,
// Volume 6, Part E, Section 1.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_ccm_bluetooth(void);

// EVP_aead_aes_128_ccm_bluetooth_8 is AES-128-CCM with M=8 and L=2 (8-byte tags
// and 13-byte nonces), as used in the Bluetooth Mesh Networking Specification
// v1.0.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_ccm_bluetooth_8(void);

// EVP_aead_aes_128_ccm_matter is AES-128-CCM with M=16 and L=2 (16-byte tags
// and 13-byte nonces), as used in the Matter specification.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_ccm_matter(void);

// EVP_has_aes_hardware returns one if we enable hardware support for fast and
// constant-time AES-GCM.
OPENSSL_EXPORT int EVP_has_aes_hardware(void);


// Utility functions.

// EVP_AEAD_key_length returns the length, in bytes, of the keys used by
// |aead|.
OPENSSL_EXPORT size_t EVP_AEAD_key_length(const EVP_AEAD *aead);

// EVP_AEAD_nonce_length returns the length, in bytes, of the per-message nonce
// for |aead|. Some |aead|s might support a larger set of nonce-lengths (e.g.
// aes-gcm).
OPENSSL_EXPORT size_t EVP_AEAD_nonce_length(const EVP_AEAD *aead);

// EVP_AEAD_max_overhead returns the maximum number of additional bytes added
// by the act of sealing data with |aead|.
OPENSSL_EXPORT size_t EVP_AEAD_max_overhead(const EVP_AEAD *aead);

// EVP_AEAD_max_tag_len returns the maximum tag length when using |aead|. This
// is the largest value that can be passed as |tag_len| to
// |EVP_AEAD_CTX_init|.
OPENSSL_EXPORT size_t EVP_AEAD_max_tag_len(const EVP_AEAD *aead);


// AEAD operations.

union evp_aead_ctx_st_state {
  uint8_t opaque[564];
  uint64_t alignment;
  void *ptr;
};

// An evp_aead_ctx_st (typedefed as |EVP_AEAD_CTX| in base.h) represents an AEAD
// algorithm configured with a specific key and message-independent IV.
struct evp_aead_ctx_st {
  const EVP_AEAD *aead;
  union evp_aead_ctx_st_state state;
  uint8_t state_offset;
  // tag_len may contain the actual length of the authentication tag if it is
  // known at initialization time.
  uint8_t tag_len;
};

// EVP_AEAD_MAX_KEY_LENGTH contains the maximum key length used by
// any AEAD defined in this header.
#define EVP_AEAD_MAX_KEY_LENGTH 80

// EVP_AEAD_MAX_NONCE_LENGTH contains the maximum nonce length used by
// any AEAD defined in this header.
#define EVP_AEAD_MAX_NONCE_LENGTH 24

// EVP_AEAD_MAX_OVERHEAD contains the maximum overhead used by any AEAD
// defined in this header.
#define EVP_AEAD_MAX_OVERHEAD 64

// EVP_AEAD_DEFAULT_TAG_LENGTH is a magic value that can be passed to
// EVP_AEAD_CTX_init to indicate that the default tag length for an AEAD should
// be used.
#define EVP_AEAD_DEFAULT_TAG_LENGTH 0

// EVP_AEAD_CTX_zero sets an uninitialized |ctx| to the zero state. It must be
// initialized with |EVP_AEAD_CTX_init| before use. It is safe, but not
// necessary, to call |EVP_AEAD_CTX_cleanup| in this state. This may be used for
// more uniform cleanup of |EVP_AEAD_CTX|.
OPENSSL_EXPORT void EVP_AEAD_CTX_zero(EVP_AEAD_CTX *ctx);

// EVP_AEAD_CTX_new allocates an |EVP_AEAD_CTX|, calls |EVP_AEAD_CTX_init| and
// returns the |EVP_AEAD_CTX|, or NULL on error.
OPENSSL_EXPORT EVP_AEAD_CTX *EVP_AEAD_CTX_new(const EVP_AEAD *aead,
                                              const uint8_t *key,
                                              size_t key_len, size_t tag_len);

// EVP_AEAD_CTX_free calls |EVP_AEAD_CTX_cleanup| and |OPENSSL_free| on
// |ctx|.
OPENSSL_EXPORT void EVP_AEAD_CTX_free(EVP_AEAD_CTX *ctx);

// EVP_AEAD_CTX_init initializes |ctx| for the given AEAD algorithm. The |impl|
// argument is ignored and should be NULL. Authentication tags may be truncated
// by passing a size as |tag_len|. A |tag_len| of zero indicates the default
// tag length and this is defined as EVP_AEAD_DEFAULT_TAG_LENGTH for
// readability.
//
// Returns 1 on success. Otherwise returns 0 and pushes to the error stack. In
// the error case, you do not need to call |EVP_AEAD_CTX_cleanup|, but it's
// harmless to do so.
OPENSSL_EXPORT int EVP_AEAD_CTX_init(EVP_AEAD_CTX *ctx, const EVP_AEAD *aead,
                                     const uint8_t *key, size_t key_len,
                                     size_t tag_len, ENGINE *impl);

// EVP_AEAD_CTX_cleanup frees any data allocated by |ctx|. It is a no-op to
// call |EVP_AEAD_CTX_cleanup| on a |EVP_AEAD_CTX| that has been |memset| to
// all zeros.
OPENSSL_EXPORT void EVP_AEAD_CTX_cleanup(EVP_AEAD_CTX *ctx);

// EVP_AEAD_CTX_seal encrypts and authenticates |in_len| bytes from |in| and
// authenticates |ad_len| bytes from |ad| and writes the result to |out|. It
// returns one on success and zero otherwise.
//
// This function may be called concurrently with itself or any other seal/open
// function on the same |EVP_AEAD_CTX|.
//
// At most |max_out_len| bytes are written to |out| and, in order to ensure
// success, |max_out_len| should be |in_len| plus the result of
// |EVP_AEAD_max_overhead|. On successful return, |*out_len| is set to the
// actual number of bytes written.
//
// The length of |nonce|, |nonce_len|, must be equal to the result of
// |EVP_AEAD_nonce_length| for this AEAD.
//
// |EVP_AEAD_CTX_seal| never results in a partial output. If |max_out_len| is
// insufficient, zero will be returned. If any error occurs, |out| will be
// filled with zero bytes and |*out_len| set to zero.
//
// If |in| and |out| alias then |out| must be == |in|.
OPENSSL_EXPORT int EVP_AEAD_CTX_seal(const EVP_AEAD_CTX *ctx, uint8_t *out,
                                     size_t *out_len, size_t max_out_len,
                                     const uint8_t *nonce, size_t nonce_len,
                                     const uint8_t *in, size_t in_len,
                                     const uint8_t *ad, size_t ad_len);

// EVP_AEAD_CTX_open authenticates |in_len| bytes from |in| and |ad_len| bytes
// from |ad| and decrypts at most |in_len| bytes into |out|. It returns one on
// success and zero otherwise.
//
// This function may be called concurrently with itself or any other seal/open
// function on the same |EVP_AEAD_CTX|.
//
// At most |in_len| bytes are written to |out|. In order to ensure success,
// |max_out_len| should be at least |in_len|. On successful return, |*out_len|
// is set to the the actual number of bytes written.
//
// The length of |nonce|, |nonce_len|, must be equal to the result of
// |EVP_AEAD_nonce_length| for this AEAD.
//
// |EVP_AEAD_CTX_open| never results in a partial output. If |max_out_len| is
// insufficient, zero will be returned. If any error occurs, |out| will be
// filled with zero bytes and |*out_len| set to zero.
//
// If |in| and |out| alias then |out| must be == |in|.
OPENSSL_EXPORT int EVP_AEAD_CTX_open(const EVP_AEAD_CTX *ctx, uint8_t *out,
                                     size_t *out_len, size_t max_out_len,
                                     const uint8_t *nonce, size_t nonce_len,
                                     const uint8_t *in, size_t in_len,
                                     const uint8_t *ad, size_t ad_len);

// EVP_AEAD_CTX_seal_scatter encrypts and authenticates |in_len| bytes from |in|
// and authenticates |ad_len| bytes from |ad|. It writes |in_len| bytes of
// ciphertext to |out| and the authentication tag to |out_tag|. It returns one
// on success and zero otherwise.
//
// This function may be called concurrently with itself or any other seal/open
// function on the same |EVP_AEAD_CTX|.
//
// Exactly |in_len| bytes are written to |out|, and up to
// |EVP_AEAD_max_overhead+extra_in_len| bytes to |out_tag|. On successful
// return, |*out_tag_len| is set to the actual number of bytes written to
// |out_tag|.
//
// |extra_in| may point to an additional plaintext input buffer if the cipher
// supports it. If present, |extra_in_len| additional bytes of plaintext are
// encrypted and authenticated, and the ciphertext is written (before the tag)
// to |out_tag|. |max_out_tag_len| must be sized to allow for the additional
// |extra_in_len| bytes.
//
// The length of |nonce|, |nonce_len|, must be equal to the result of
// |EVP_AEAD_nonce_length| for this AEAD.
//
// |EVP_AEAD_CTX_seal_scatter| never results in a partial output. If
// |max_out_tag_len| is insufficient, zero will be returned. If any error
// occurs, |out| and |out_tag| will be filled with zero bytes and |*out_tag_len|
// set to zero.
//
// If |in| and |out| alias then |out| must be == |in|. |out_tag| may not alias
// any other argument.
OPENSSL_EXPORT int EVP_AEAD_CTX_seal_scatter(
    const EVP_AEAD_CTX *ctx, uint8_t *out, uint8_t *out_tag,
    size_t *out_tag_len, size_t max_out_tag_len, const uint8_t *nonce,
    size_t nonce_len, const uint8_t *in, size_t in_len, const uint8_t *extra_in,
    size_t extra_in_len, const uint8_t *ad, size_t ad_len);

// EVP_AEAD_CTX_open_gather decrypts and authenticates |in_len| bytes from |in|
// and authenticates |ad_len| bytes from |ad| using |in_tag_len| bytes of
// authentication tag from |in_tag|. If successful, it writes |in_len| bytes of
// plaintext to |out|. It returns one on success and zero otherwise.
//
// This function may be called concurrently with itself or any other seal/open
// function on the same |EVP_AEAD_CTX|.
//
// The length of |nonce|, |nonce_len|, must be equal to the result of
// |EVP_AEAD_nonce_length| for this AEAD.
//
// |EVP_AEAD_CTX_open_gather| never results in a partial output. If any error
// occurs, |out| will be filled with zero bytes.
//
// If |in| and |out| alias then |out| must be == |in|.
OPENSSL_EXPORT int EVP_AEAD_CTX_open_gather(
    const EVP_AEAD_CTX *ctx, uint8_t *out, const uint8_t *nonce,
    size_t nonce_len, const uint8_t *in, size_t in_len, const uint8_t *in_tag,
    size_t in_tag_len, const uint8_t *ad, size_t ad_len);

// EVP_AEAD_CTX_aead returns the underlying AEAD for |ctx|, or NULL if one has
// not been set.
OPENSSL_EXPORT const EVP_AEAD *EVP_AEAD_CTX_aead(const EVP_AEAD_CTX *ctx);


// TLS-specific AEAD algorithms.
//
// These AEAD primitives do not meet the definition of generic AEADs. They are
// all specific to TLS and should not be used outside of that context. They must
// be initialized with |EVP_AEAD_CTX_init_with_direction|, are stateful, and may
// not be used concurrently. Any nonces are used as IVs, so they must be
// unpredictable. They only accept an |ad| parameter of length 11 (the standard
// TLS one with length omitted).

OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_cbc_sha1_tls(void);
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_cbc_sha1_tls_implicit_iv(void);

OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_cbc_sha1_tls(void);
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_cbc_sha1_tls_implicit_iv(void);

OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_cbc_sha256_tls(void);
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_cbc_sha256_tls_implicit_iv(
    void);

OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_cbc_sha384_tls(void);

OPENSSL_EXPORT const EVP_AEAD *EVP_aead_des_ede3_cbc_sha1_tls(void);
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_des_ede3_cbc_sha1_tls_implicit_iv(void);

OPENSSL_EXPORT const EVP_AEAD *EVP_aead_null_sha1_tls(void);

// EVP_aead_aes_128_gcm_tls12 is AES-128 in Galois Counter Mode using the TLS
// 1.2 nonce construction.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_gcm_tls12(void);

// EVP_aead_aes_256_gcm_tls12 is AES-256 in Galois Counter Mode using the TLS
// 1.2 nonce construction.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_gcm_tls12(void);

// EVP_aead_aes_128_gcm_tls13 is AES-128 in Galois Counter Mode using the TLS
// 1.3 nonce construction.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_128_gcm_tls13(void);

// EVP_aead_aes_256_gcm_tls13 is AES-256 in Galois Counter Mode using the TLS
// 1.3 nonce construction.
OPENSSL_EXPORT const EVP_AEAD *EVP_aead_aes_256_gcm_tls13(void);


// Obscure functions.

// evp_aead_direction_t denotes the direction of an AEAD operation.
enum evp_aead_direction_t {
  evp_aead_open,
  evp_aead_seal
};

// EVP_AEAD_CTX_init_with_direction calls |EVP_AEAD_CTX_init| for normal
// AEADs. For TLS-specific and SSL3-specific AEADs, it initializes |ctx| for a
// given direction.
OPENSSL_EXPORT int EVP_AEAD_CTX_init_with_direction(
    EVP_AEAD_CTX *ctx, const EVP_AEAD *aead, const uint8_t *key, size_t key_len,
    size_t tag_len, enum evp_aead_direction_t dir);

// EVP_AEAD_CTX_get_iv sets |*out_len| to the length of the IV for |ctx| and
// sets |*out_iv| to point to that many bytes of the current IV. This is only
// meaningful for AEADs with implicit IVs (i.e. CBC mode in TLS 1.0).
//
// It returns one on success or zero on error.
OPENSSL_EXPORT int EVP_AEAD_CTX_get_iv(const EVP_AEAD_CTX *ctx,
                                       const uint8_t **out_iv, size_t *out_len);

// EVP_AEAD_CTX_tag_len computes the exact byte length of the tag written by
// |EVP_AEAD_CTX_seal_scatter| and writes it to |*out_tag_len|. It returns one
// on success or zero on error. |in_len| and |extra_in_len| must equal the
// arguments of the same names passed to |EVP_AEAD_CTX_seal_scatter|.
OPENSSL_EXPORT int EVP_AEAD_CTX_tag_len(const EVP_AEAD_CTX *ctx,
                                        size_t *out_tag_len,
                                        const size_t in_len,
                                        const size_t extra_in_len);

#define FIPS_AES_GCM_NONCE_LENGTH 12

// EVP_AEAD_get_iv_from_ipv4_nanosecs computes a deterministic IV compliant with
// NIST SP 800-38D, built from an IPv4 address and the number of nanoseconds
// since boot, writing it to |out_iv| (in little endian).
// It returns one on success or zero for error.
//
// This is not a general-purpose API, you should not be using it unless you
// specifically know you need to use this.
OPENSSL_EXPORT int EVP_AEAD_get_iv_from_ipv4_nanosecs(
    const uint32_t ipv4_address, const uint64_t nanosecs,
    uint8_t out_iv[FIPS_AES_GCM_NONCE_LENGTH]);


#if defined(__cplusplus)
}  // extern C

#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

using ScopedEVP_AEAD_CTX =
    internal::StackAllocated<EVP_AEAD_CTX, void, EVP_AEAD_CTX_zero,
                             EVP_AEAD_CTX_cleanup>;

BORINGSSL_MAKE_DELETER(EVP_AEAD_CTX, EVP_AEAD_CTX_free)

BSSL_NAMESPACE_END

}  // extern C++
#endif

#endif

#endif  // OPENSSL_HEADER_AEAD_H
