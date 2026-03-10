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

#ifndef OPENSSL_HEADER_BYTESTRING_H
#define OPENSSL_HEADER_BYTESTRING_H

#include <openssl/base.h>

#include <openssl/span.h>
#include <time.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Bytestrings are used for parsing and building TLS and ASN.1 messages.
//
// A "CBS" (CRYPTO ByteString) represents a string of bytes in memory and
// provides utility functions for safely parsing length-prefixed structures
// like TLS and ASN.1 from it.
//
// A "CBB" (CRYPTO ByteBuilder) is a memory buffer that grows as needed and
// provides utility functions for building length-prefixed messages.


// CRYPTO ByteString

struct cbs_st {
  const uint8_t *data;
  size_t len;

#if !defined(BORINGSSL_NO_CXX)
  // Allow implicit conversions to and from bssl::Span<const uint8_t>.
  cbs_st(bssl::Span<const uint8_t> span)
      : data(span.data()), len(span.size()) {}
  operator bssl::Span<const uint8_t>() const {
    return bssl::MakeConstSpan(data, len);
  }

  // Defining any constructors requires we explicitly default the others.
  cbs_st() = default;
  cbs_st(const cbs_st &) = default;
  cbs_st &operator=(const cbs_st &) = default;
#endif
};

// CBS_init sets |cbs| to point to |data|. It does not take ownership of
// |data|.
OPENSSL_EXPORT void CBS_init(CBS *cbs, const uint8_t *data, size_t len);

// CBS_skip advances |cbs| by |len| bytes. It returns one on success and zero
// otherwise.
OPENSSL_EXPORT int CBS_skip(CBS *cbs, size_t len);

// CBS_data returns a pointer to the contents of |cbs|.
OPENSSL_EXPORT const uint8_t *CBS_data(const CBS *cbs);

// CBS_len returns the number of bytes remaining in |cbs|.
OPENSSL_EXPORT size_t CBS_len(const CBS *cbs);

// CBS_stow copies the current contents of |cbs| into |*out_ptr| and
// |*out_len|. If |*out_ptr| is not NULL, the contents are freed with
// OPENSSL_free. It returns one on success and zero on allocation failure. On
// success, |*out_ptr| should be freed with OPENSSL_free. If |cbs| is empty,
// |*out_ptr| will be NULL.
OPENSSL_EXPORT int CBS_stow(const CBS *cbs, uint8_t **out_ptr, size_t *out_len);

// CBS_strdup copies the current contents of |cbs| into |*out_ptr| as a
// NUL-terminated C string. If |*out_ptr| is not NULL, the contents are freed
// with OPENSSL_free. It returns one on success and zero on allocation
// failure. On success, |*out_ptr| should be freed with OPENSSL_free.
//
// NOTE: If |cbs| contains NUL bytes, the string will be truncated. Call
// |CBS_contains_zero_byte(cbs)| to check for NUL bytes.
OPENSSL_EXPORT int CBS_strdup(const CBS *cbs, char **out_ptr);

// CBS_contains_zero_byte returns one if the current contents of |cbs| contains
// a NUL byte and zero otherwise.
OPENSSL_EXPORT int CBS_contains_zero_byte(const CBS *cbs);

// CBS_mem_equal compares the current contents of |cbs| with the |len| bytes
// starting at |data|. If they're equal, it returns one, otherwise zero. If the
// lengths match, it uses a constant-time comparison.
OPENSSL_EXPORT int CBS_mem_equal(const CBS *cbs, const uint8_t *data,
                                 size_t len);

// CBS_get_u8 sets |*out| to the next uint8_t from |cbs| and advances |cbs|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u8(CBS *cbs, uint8_t *out);

// CBS_get_u16 sets |*out| to the next, big-endian uint16_t from |cbs| and
// advances |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u16(CBS *cbs, uint16_t *out);

// CBS_get_u16le sets |*out| to the next, little-endian uint16_t from |cbs| and
// advances |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u16le(CBS *cbs, uint16_t *out);

// CBS_get_u24 sets |*out| to the next, big-endian 24-bit value from |cbs| and
// advances |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u24(CBS *cbs, uint32_t *out);

// CBS_get_u32 sets |*out| to the next, big-endian uint32_t value from |cbs|
// and advances |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u32(CBS *cbs, uint32_t *out);

// CBS_get_u32le sets |*out| to the next, little-endian uint32_t value from
// |cbs| and advances |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u32le(CBS *cbs, uint32_t *out);

// CBS_get_u64 sets |*out| to the next, big-endian uint64_t value from |cbs|
// and advances |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u64(CBS *cbs, uint64_t *out);

// CBS_get_u64le sets |*out| to the next, little-endian uint64_t value from
// |cbs| and advances |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u64le(CBS *cbs, uint64_t *out);

// CBS_get_last_u8 sets |*out| to the last uint8_t from |cbs| and shortens
// |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_last_u8(CBS *cbs, uint8_t *out);

// CBS_get_bytes sets |*out| to the next |len| bytes from |cbs| and advances
// |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_bytes(CBS *cbs, CBS *out, size_t len);

// CBS_copy_bytes copies the next |len| bytes from |cbs| to |out| and advances
// |cbs|. It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_copy_bytes(CBS *cbs, uint8_t *out, size_t len);

// CBS_get_u8_length_prefixed sets |*out| to the contents of an 8-bit,
// length-prefixed value from |cbs| and advances |cbs| over it. It returns one
// on success and zero on error.
OPENSSL_EXPORT int CBS_get_u8_length_prefixed(CBS *cbs, CBS *out);

// CBS_get_u16_length_prefixed sets |*out| to the contents of a 16-bit,
// big-endian, length-prefixed value from |cbs| and advances |cbs| over it. It
// returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u16_length_prefixed(CBS *cbs, CBS *out);

// CBS_get_u24_length_prefixed sets |*out| to the contents of a 24-bit,
// big-endian, length-prefixed value from |cbs| and advances |cbs| over it. It
// returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_u24_length_prefixed(CBS *cbs, CBS *out);

// CBS_get_until_first finds the first instance of |c| in |cbs|. If found, it
// sets |*out| to the text before the match, advances |cbs| over it, and returns
// one. Otherwise, it returns zero and leaves |cbs| unmodified.
OPENSSL_EXPORT int CBS_get_until_first(CBS *cbs, CBS *out, uint8_t c);

// CBS_get_u64_decimal reads a decimal integer from |cbs| and writes it to
// |*out|. It stops reading at the end of the string, or the first non-digit
// character. It returns one on success and zero on error. This function behaves
// analogously to |strtoul| except it does not accept empty inputs, leading
// zeros, or negative values.
OPENSSL_EXPORT int CBS_get_u64_decimal(CBS *cbs, uint64_t *out);


// Parsing ASN.1
//
// |CBS| may be used to parse DER structures. Rather than using a schema
// compiler, the following functions act on tag-length-value elements in the
// serialization itself. Thus the caller is responsible for looping over a
// SEQUENCE, branching on CHOICEs or OPTIONAL fields, checking for trailing
// data, and handling explict vs. implicit tagging.
//
// Tags are represented as |CBS_ASN1_TAG| values in memory. The upper few bits
// store the class and constructed bit, and the remaining bits store the tag
// number. Note this differs from the DER serialization, to support tag numbers
// beyond 31. Consumers must use the constants defined below to decompose or
// assemble tags.
//
// This library treats an element's constructed bit as part of its tag. In DER,
// the constructed bit is computable from the type. The constants for universal
// types have the bit set. Callers must set it correctly for tagged types.
// Explicitly-tagged types are always constructed, and implicitly-tagged types
// inherit the underlying type's bit.

// CBS_ASN1_TAG_SHIFT is how much the in-memory representation shifts the class
// and constructed bits from the DER serialization.
#define CBS_ASN1_TAG_SHIFT 24

// CBS_ASN1_CONSTRUCTED may be ORed into a tag to set the constructed bit.
#define CBS_ASN1_CONSTRUCTED (0x20u << CBS_ASN1_TAG_SHIFT)

// The following values specify the tag class and may be ORed into a tag number
// to produce the final tag. If none is used, the tag will be UNIVERSAL.
#define CBS_ASN1_UNIVERSAL (0u << CBS_ASN1_TAG_SHIFT)
#define CBS_ASN1_APPLICATION (0x40u << CBS_ASN1_TAG_SHIFT)
#define CBS_ASN1_CONTEXT_SPECIFIC (0x80u << CBS_ASN1_TAG_SHIFT)
#define CBS_ASN1_PRIVATE (0xc0u << CBS_ASN1_TAG_SHIFT)

// CBS_ASN1_CLASS_MASK may be ANDed with a tag to query its class. This will
// give one of the four values above.
#define CBS_ASN1_CLASS_MASK (0xc0u << CBS_ASN1_TAG_SHIFT)

// CBS_ASN1_TAG_NUMBER_MASK may be ANDed with a tag to query its number.
#define CBS_ASN1_TAG_NUMBER_MASK ((1u << (5 + CBS_ASN1_TAG_SHIFT)) - 1)

// The following values are constants for UNIVERSAL tags. Note these constants
// include the constructed bit.
#define CBS_ASN1_BOOLEAN 0x1u
#define CBS_ASN1_INTEGER 0x2u
#define CBS_ASN1_BITSTRING 0x3u
#define CBS_ASN1_OCTETSTRING 0x4u
#define CBS_ASN1_NULL 0x5u
#define CBS_ASN1_OBJECT 0x6u
#define CBS_ASN1_ENUMERATED 0xau
#define CBS_ASN1_UTF8STRING 0xcu
#define CBS_ASN1_SEQUENCE (0x10u | CBS_ASN1_CONSTRUCTED)
#define CBS_ASN1_SET (0x11u | CBS_ASN1_CONSTRUCTED)
#define CBS_ASN1_NUMERICSTRING 0x12u
#define CBS_ASN1_PRINTABLESTRING 0x13u
#define CBS_ASN1_T61STRING 0x14u
#define CBS_ASN1_VIDEOTEXSTRING 0x15u
#define CBS_ASN1_IA5STRING 0x16u
#define CBS_ASN1_UTCTIME 0x17u
#define CBS_ASN1_GENERALIZEDTIME 0x18u
#define CBS_ASN1_GRAPHICSTRING 0x19u
#define CBS_ASN1_VISIBLESTRING 0x1au
#define CBS_ASN1_GENERALSTRING 0x1bu
#define CBS_ASN1_UNIVERSALSTRING 0x1cu
#define CBS_ASN1_BMPSTRING 0x1eu

// CBS_get_asn1 sets |*out| to the contents of DER-encoded, ASN.1 element (not
// including tag and length bytes) and advances |cbs| over it. The ASN.1
// element must match |tag_value|.
//
// If |*out| is NULL, then the contents will be discarded.
//
// It returns one on success and zero on error.
OPENSSL_EXPORT int CBS_get_asn1(CBS *cbs, CBS *out, CBS_ASN1_TAG tag_value);

// CBS_get_asn1_element acts like |CBS_get_asn1| but |out| will include the
// ASN.1 header bytes too.
OPENSSL_EXPORT int CBS_get_asn1_element(CBS *cbs, CBS *out,
                                        CBS_ASN1_TAG tag_value);

// CBS_peek_asn1_tag looks ahead at the next ASN.1 tag and returns one
// if the next ASN.1 element on |cbs| would have tag |tag_value|. If
// |cbs| is empty or the tag does not match, it returns zero. Note: if
// it returns one, CBS_get_asn1 may still fail if the rest of the
// element is malformed.
OPENSSL_EXPORT int CBS_peek_asn1_tag(const CBS *cbs, CBS_ASN1_TAG tag_value);

// CBS_get_any_asn1 sets |*out| to contain the next ASN.1 element from |*cbs|
// (not including tag and length bytes), sets |*out_tag| to the tag number, and
// advances |*cbs|. It returns one on success and zero on error. Either of |out|
// and |out_tag| may be NULL to ignore the value.
OPENSSL_EXPORT int CBS_get_any_asn1(CBS *cbs, CBS *out, CBS_ASN1_TAG *out_tag);

// CBS_get_any_asn1_element sets |*out| to contain the next ASN.1 element from
// |*cbs| (including header bytes) and advances |*cbs|. It sets |*out_tag| to
// the tag number and |*out_header_len| to the length of the ASN.1 header. Each
// of |out|, |out_tag|, and |out_header_len| may be NULL to ignore the value.
OPENSSL_EXPORT int CBS_get_any_asn1_element(CBS *cbs, CBS *out,
                                            CBS_ASN1_TAG *out_tag,
                                            size_t *out_header_len);

// CBS_get_any_ber_asn1_element acts the same as |CBS_get_any_asn1_element| but
// also allows indefinite-length elements to be returned and does not enforce
// that lengths are minimal. It sets |*out_indefinite| to one if the length was
// indefinite and zero otherwise. If indefinite, |*out_header_len| and
// |CBS_len(out)| will be equal as only the header is returned (although this is
// also true for empty elements so |*out_indefinite| should be checked). If
// |out_ber_found| is not NULL then it is set to one if any case of invalid DER
// but valid BER is found, and to zero otherwise.
//
// This function will not successfully parse an end-of-contents (EOC) as an
// element. Callers parsing indefinite-length encoding must check for EOC
// separately.
OPENSSL_EXPORT int CBS_get_any_ber_asn1_element(CBS *cbs, CBS *out,
                                                CBS_ASN1_TAG *out_tag,
                                                size_t *out_header_len,
                                                int *out_ber_found,
                                                int *out_indefinite);

// CBS_get_asn1_uint64 gets an ASN.1 INTEGER from |cbs| using |CBS_get_asn1|
// and sets |*out| to its value. It returns one on success and zero on error,
// where error includes the integer being negative, or too large to represent
// in 64 bits.
OPENSSL_EXPORT int CBS_get_asn1_uint64(CBS *cbs, uint64_t *out);

// CBS_get_asn1_int64 gets an ASN.1 INTEGER from |cbs| using |CBS_get_asn1|
// and sets |*out| to its value. It returns one on success and zero on error,
// where error includes the integer being too large to represent in 64 bits.
OPENSSL_EXPORT int CBS_get_asn1_int64(CBS *cbs, int64_t *out);

// CBS_get_asn1_bool gets an ASN.1 BOOLEAN from |cbs| and sets |*out| to zero
// or one based on its value. It returns one on success or zero on error.
OPENSSL_EXPORT int CBS_get_asn1_bool(CBS *cbs, int *out);

// CBS_get_optional_asn1 gets an optional explicitly-tagged element from |cbs|
// tagged with |tag| and sets |*out| to its contents, or ignores it if |out| is
// NULL. If present and if |out_present| is not NULL, it sets |*out_present| to
// one, otherwise zero. It returns one on success, whether or not the element
// was present, and zero on decode failure.
OPENSSL_EXPORT int CBS_get_optional_asn1(CBS *cbs, CBS *out, int *out_present,
                                         CBS_ASN1_TAG tag);

// CBS_get_optional_asn1_octet_string gets an optional
// explicitly-tagged OCTET STRING from |cbs|. If present, it sets
// |*out| to the string and |*out_present| to one. Otherwise, it sets
// |*out| to empty and |*out_present| to zero. |out_present| may be
// NULL. It returns one on success, whether or not the element was
// present, and zero on decode failure.
OPENSSL_EXPORT int CBS_get_optional_asn1_octet_string(CBS *cbs, CBS *out,
                                                      int *out_present,
                                                      CBS_ASN1_TAG tag);

// CBS_get_optional_asn1_uint64 gets an optional explicitly-tagged
// INTEGER from |cbs|. If present, it sets |*out| to the
// value. Otherwise, it sets |*out| to |default_value|. It returns one
// on success, whether or not the element was present, and zero on
// decode failure.
OPENSSL_EXPORT int CBS_get_optional_asn1_uint64(CBS *cbs, uint64_t *out,
                                                CBS_ASN1_TAG tag,
                                                uint64_t default_value);

// CBS_get_optional_asn1_bool gets an optional, explicitly-tagged BOOLEAN from
// |cbs|. If present, it sets |*out| to either zero or one, based on the
// boolean. Otherwise, it sets |*out| to |default_value|. It returns one on
// success, whether or not the element was present, and zero on decode
// failure.
OPENSSL_EXPORT int CBS_get_optional_asn1_bool(CBS *cbs, int *out,
                                              CBS_ASN1_TAG tag,
                                              int default_value);

// CBS_is_valid_asn1_bitstring returns one if |cbs| is a valid ASN.1 BIT STRING
// body and zero otherwise.
OPENSSL_EXPORT int CBS_is_valid_asn1_bitstring(const CBS *cbs);

// CBS_asn1_bitstring_has_bit returns one if |cbs| is a valid ASN.1 BIT STRING
// body and the specified bit is present and set. Otherwise, it returns zero.
// |bit| is indexed starting from zero.
OPENSSL_EXPORT int CBS_asn1_bitstring_has_bit(const CBS *cbs, unsigned bit);

// CBS_is_valid_asn1_integer returns one if |cbs| is a valid ASN.1 INTEGER,
// body and zero otherwise. On success, if |out_is_negative| is non-NULL,
// |*out_is_negative| will be set to one if |cbs| is negative and zero
// otherwise.
OPENSSL_EXPORT int CBS_is_valid_asn1_integer(const CBS *cbs,
                                             int *out_is_negative);

// CBS_is_unsigned_asn1_integer returns one if |cbs| is a valid non-negative
// ASN.1 INTEGER body and zero otherwise.
OPENSSL_EXPORT int CBS_is_unsigned_asn1_integer(const CBS *cbs);

// CBS_is_valid_asn1_oid returns one if |cbs| is a valid DER-encoded ASN.1
// OBJECT IDENTIFIER contents (not including the element framing) and zero
// otherwise. This function tolerates arbitrarily large OID components.
OPENSSL_EXPORT int CBS_is_valid_asn1_oid(const CBS *cbs);

// CBS_asn1_oid_to_text interprets |cbs| as DER-encoded ASN.1 OBJECT IDENTIFIER
// contents (not including the element framing) and returns the ASCII
// representation (e.g., "1.2.840.113554.4.1.72585") in a newly-allocated
// string, or NULL on failure. The caller must release the result with
// |OPENSSL_free|.
//
// This function may fail if |cbs| is an invalid OBJECT IDENTIFIER, or if any
// OID components are too large.
OPENSSL_EXPORT char *CBS_asn1_oid_to_text(const CBS *cbs);


// CBS_parse_generalized_time returns one if |cbs| is a valid DER-encoded, ASN.1
// GeneralizedTime body within the limitations imposed by RFC 5280, or zero
// otherwise. If |allow_timezone_offset| is non-zero, four-digit timezone
// offsets, which would not be allowed by DER, are permitted. On success, if
// |out_tm| is non-NULL, |*out_tm| will be zeroed, and then set to the
// corresponding time in UTC. This function does not compute |out_tm->tm_wday|
// or |out_tm->tm_yday|.
OPENSSL_EXPORT int CBS_parse_generalized_time(const CBS *cbs, struct tm *out_tm,
                                              int allow_timezone_offset);

// CBS_parse_utc_time returns one if |cbs| is a valid DER-encoded, ASN.1
// UTCTime body within the limitations imposed by RFC 5280, or zero otherwise.
// If |allow_timezone_offset| is non-zero, four-digit timezone offsets, which
// would not be allowed by DER, are permitted. On success, if |out_tm| is
// non-NULL, |*out_tm| will be zeroed, and then set to the corresponding time
// in UTC. This function does not compute |out_tm->tm_wday| or
// |out_tm->tm_yday|.
OPENSSL_EXPORT int CBS_parse_utc_time(const CBS *cbs, struct tm *out_tm,
                                      int allow_timezone_offset);

// CBS_get_optional_asn1_int64 gets an optional, explicitly-tagged INTEGER from
// |cbs|. If present, it sets |*out| to the value. Otherwise, it sets |*out| to
// |default_value|. It returns one on success and zero on error, where error
// includes the integer being too large to represent in 64 bits.
OPENSSL_EXPORT int CBS_get_optional_asn1_int64(CBS *cbs, int64_t *out,
                                               CBS_ASN1_TAG tag,
                                               int64_t default_value);

// CRYPTO ByteBuilder.
//
// |CBB| objects allow one to build length-prefixed serialisations. A |CBB|
// object is associated with a buffer and new buffers are created with
// |CBB_init|. Several |CBB| objects can point at the same buffer when a
// length-prefix is pending, however only a single |CBB| can be 'current' at
// any one time. For example, if one calls |CBB_add_u8_length_prefixed| then
// the new |CBB| points at the same buffer as the original. But if the original
// |CBB| is used then the length prefix is written out and the new |CBB| must
// not be used again.
//
// If one needs to force a length prefix to be written out because a |CBB| is
// going out of scope, use |CBB_flush|. If an operation on a |CBB| fails, it is
// in an undefined state and must not be used except to call |CBB_cleanup|.

struct cbb_buffer_st {
  uint8_t *buf;
  // len is the number of valid bytes in |buf|.
  size_t len;
  // cap is the size of |buf|.
  size_t cap;
  // can_resize is one iff |buf| is owned by this object. If not then |buf|
  // cannot be resized.
  unsigned can_resize : 1;
  // error is one if there was an error writing to this CBB. All future
  // operations will fail.
  unsigned error : 1;
};

struct cbb_child_st {
  // base is a pointer to the buffer this |CBB| writes to.
  struct cbb_buffer_st *base;
  // offset is the number of bytes from the start of |base->buf| to this |CBB|'s
  // pending length prefix.
  size_t offset;
  // pending_len_len contains the number of bytes in this |CBB|'s pending
  // length-prefix, or zero if no length-prefix is pending.
  uint8_t pending_len_len;
  unsigned pending_is_asn1 : 1;
};

struct cbb_st {
  // child points to a child CBB if a length-prefix is pending.
  CBB *child;
  // is_child is one if this is a child |CBB| and zero if it is a top-level
  // |CBB|. This determines which arm of the union is valid.
  char is_child;
  union {
    struct cbb_buffer_st base;
    struct cbb_child_st child;
  } u;
};

// CBB_zero sets an uninitialised |cbb| to the zero state. It must be
// initialised with |CBB_init| or |CBB_init_fixed| before use, but it is safe to
// call |CBB_cleanup| without a successful |CBB_init|. This may be used for more
// uniform cleanup of a |CBB|.
OPENSSL_EXPORT void CBB_zero(CBB *cbb);

// CBB_init initialises |cbb| with |initial_capacity|. Since a |CBB| grows as
// needed, the |initial_capacity| is just a hint. It returns one on success or
// zero on allocation failure.
OPENSSL_EXPORT int CBB_init(CBB *cbb, size_t initial_capacity);

// CBB_init_fixed initialises |cbb| to write to |len| bytes at |buf|. Since
// |buf| cannot grow, trying to write more than |len| bytes will cause CBB
// functions to fail. This function is infallible and always returns one. It is
// safe, but not necessary, to call |CBB_cleanup| on |cbb|.
OPENSSL_EXPORT int CBB_init_fixed(CBB *cbb, uint8_t *buf, size_t len);

// CBB_cleanup frees all resources owned by |cbb| and other |CBB| objects
// writing to the same buffer. This should be used in an error case where a
// serialisation is abandoned.
//
// This function can only be called on a "top level" |CBB|, i.e. one initialised
// with |CBB_init| or |CBB_init_fixed|, or a |CBB| set to the zero state with
// |CBB_zero|.
OPENSSL_EXPORT void CBB_cleanup(CBB *cbb);

// CBB_finish completes any pending length prefix and sets |*out_data| to a
// malloced buffer and |*out_len| to the length of that buffer. The caller
// takes ownership of the buffer and, unless the buffer was fixed with
// |CBB_init_fixed|, must call |OPENSSL_free| when done.
//
// It can only be called on a "top level" |CBB|, i.e. one initialised with
// |CBB_init| or |CBB_init_fixed|. It returns one on success and zero on
// error.
OPENSSL_EXPORT int CBB_finish(CBB *cbb, uint8_t **out_data, size_t *out_len);

// CBB_flush causes any pending length prefixes to be written out and any child
// |CBB| objects of |cbb| to be invalidated. This allows |cbb| to continue to be
// used after the children go out of scope, e.g. when local |CBB| objects are
// added as children to a |CBB| that persists after a function returns. This
// function returns one on success or zero on error.
OPENSSL_EXPORT int CBB_flush(CBB *cbb);

// CBB_data returns a pointer to the bytes written to |cbb|. It does not flush
// |cbb|. The pointer is valid until the next operation to |cbb|.
//
// To avoid unfinalized length prefixes, it is a fatal error to call this on a
// CBB with any active children.
OPENSSL_EXPORT const uint8_t *CBB_data(const CBB *cbb);

// CBB_len returns the number of bytes written to |cbb|. It does not flush
// |cbb|.
//
// To avoid unfinalized length prefixes, it is a fatal error to call this on a
// CBB with any active children.
OPENSSL_EXPORT size_t CBB_len(const CBB *cbb);

// CBB_add_u8_length_prefixed sets |*out_contents| to a new child of |cbb|. The
// data written to |*out_contents| will be prefixed in |cbb| with an 8-bit
// length. It returns one on success or zero on error.
OPENSSL_EXPORT int CBB_add_u8_length_prefixed(CBB *cbb, CBB *out_contents);

// CBB_add_u16_length_prefixed sets |*out_contents| to a new child of |cbb|.
// The data written to |*out_contents| will be prefixed in |cbb| with a 16-bit,
// big-endian length. It returns one on success or zero on error.
OPENSSL_EXPORT int CBB_add_u16_length_prefixed(CBB *cbb, CBB *out_contents);

// CBB_add_u24_length_prefixed sets |*out_contents| to a new child of |cbb|.
// The data written to |*out_contents| will be prefixed in |cbb| with a 24-bit,
// big-endian length. It returns one on success or zero on error.
OPENSSL_EXPORT int CBB_add_u24_length_prefixed(CBB *cbb, CBB *out_contents);

// CBB_add_asn1 sets |*out_contents| to a |CBB| into which the contents of an
// ASN.1 object can be written. The |tag| argument will be used as the tag for
// the object. It returns one on success or zero on error.
OPENSSL_EXPORT int CBB_add_asn1(CBB *cbb, CBB *out_contents, CBS_ASN1_TAG tag);

// CBB_add_bytes appends |len| bytes from |data| to |cbb|. It returns one on
// success and zero otherwise.
OPENSSL_EXPORT int CBB_add_bytes(CBB *cbb, const uint8_t *data, size_t len);

// CBB_add_zeros append |len| bytes with value zero to |cbb|. It returns one on
// success and zero otherwise.
OPENSSL_EXPORT int CBB_add_zeros(CBB *cbb, size_t len);

// CBB_add_space appends |len| bytes to |cbb| and sets |*out_data| to point to
// the beginning of that space. The caller must then write |len| bytes of
// actual contents to |*out_data|. It returns one on success and zero
// otherwise.
OPENSSL_EXPORT int CBB_add_space(CBB *cbb, uint8_t **out_data, size_t len);

// CBB_reserve ensures |cbb| has room for |len| additional bytes and sets
// |*out_data| to point to the beginning of that space. It returns one on
// success and zero otherwise. The caller may write up to |len| bytes to
// |*out_data| and call |CBB_did_write| to complete the write. |*out_data| is
// valid until the next operation on |cbb| or an ancestor |CBB|.
OPENSSL_EXPORT int CBB_reserve(CBB *cbb, uint8_t **out_data, size_t len);

// CBB_did_write advances |cbb| by |len| bytes, assuming the space has been
// written to by the caller. It returns one on success and zero on error.
OPENSSL_EXPORT int CBB_did_write(CBB *cbb, size_t len);

// CBB_add_u8 appends an 8-bit number from |value| to |cbb|. It returns one on
// success and zero otherwise.
OPENSSL_EXPORT int CBB_add_u8(CBB *cbb, uint8_t value);

// CBB_add_u16 appends a 16-bit, big-endian number from |value| to |cbb|. It
// returns one on success and zero otherwise.
OPENSSL_EXPORT int CBB_add_u16(CBB *cbb, uint16_t value);

// CBB_add_u16le appends a 16-bit, little-endian number from |value| to |cbb|.
// It returns one on success and zero otherwise.
OPENSSL_EXPORT int CBB_add_u16le(CBB *cbb, uint16_t value);

// CBB_add_u24 appends a 24-bit, big-endian number from |value| to |cbb|. It
// returns one on success and zero otherwise.
OPENSSL_EXPORT int CBB_add_u24(CBB *cbb, uint32_t value);

// CBB_add_u32 appends a 32-bit, big-endian number from |value| to |cbb|. It
// returns one on success and zero otherwise.
OPENSSL_EXPORT int CBB_add_u32(CBB *cbb, uint32_t value);

// CBB_add_u32le appends a 32-bit, little-endian number from |value| to |cbb|.
// It returns one on success and zero otherwise.
OPENSSL_EXPORT int CBB_add_u32le(CBB *cbb, uint32_t value);

// CBB_add_u64 appends a 64-bit, big-endian number from |value| to |cbb|. It
// returns one on success and zero otherwise.
OPENSSL_EXPORT int CBB_add_u64(CBB *cbb, uint64_t value);

// CBB_add_u64le appends a 64-bit, little-endian number from |value| to |cbb|.
// It returns one on success and zero otherwise.
OPENSSL_EXPORT int CBB_add_u64le(CBB *cbb, uint64_t value);

// CBB_discard_child discards the current unflushed child of |cbb|. Neither the
// child's contents nor the length prefix will be included in the output.
OPENSSL_EXPORT void CBB_discard_child(CBB *cbb);

// CBB_add_asn1_uint64 writes an ASN.1 INTEGER into |cbb| using |CBB_add_asn1|
// and writes |value| in its contents. It returns one on success and zero on
// error.
OPENSSL_EXPORT int CBB_add_asn1_uint64(CBB *cbb, uint64_t value);

// CBB_add_asn1_uint64_with_tag behaves like |CBB_add_asn1_uint64| but uses
// |tag| as the tag instead of INTEGER. This is useful if the INTEGER type uses
// implicit tagging.
OPENSSL_EXPORT int CBB_add_asn1_uint64_with_tag(CBB *cbb, uint64_t value,
                                                CBS_ASN1_TAG tag);

// CBB_add_asn1_int64 writes an ASN.1 INTEGER into |cbb| using |CBB_add_asn1|
// and writes |value| in its contents. It returns one on success and zero on
// error.
OPENSSL_EXPORT int CBB_add_asn1_int64(CBB *cbb, int64_t value);

// CBB_add_asn1_int64_with_tag behaves like |CBB_add_asn1_int64| but uses |tag|
// as the tag instead of INTEGER. This is useful if the INTEGER type uses
// implicit tagging.
OPENSSL_EXPORT int CBB_add_asn1_int64_with_tag(CBB *cbb, int64_t value,
                                               CBS_ASN1_TAG tag);

// CBB_add_asn1_octet_string writes an ASN.1 OCTET STRING into |cbb| with the
// given contents. It returns one on success and zero on error.
OPENSSL_EXPORT int CBB_add_asn1_octet_string(CBB *cbb, const uint8_t *data,
                                             size_t data_len);

// CBB_add_asn1_bool writes an ASN.1 BOOLEAN into |cbb| which is true iff
// |value| is non-zero.  It returns one on success and zero on error.
OPENSSL_EXPORT int CBB_add_asn1_bool(CBB *cbb, int value);

// CBB_add_asn1_oid_from_text decodes |len| bytes from |text| as an ASCII OID
// representation, e.g. "1.2.840.113554.4.1.72585", and writes the DER-encoded
// contents to |cbb|. It returns one on success and zero on malloc failure or if
// |text| was invalid. It does not include the OBJECT IDENTIFER framing, only
// the element's contents.
//
// This function considers OID strings with components which do not fit in a
// |uint64_t| to be invalid.
OPENSSL_EXPORT int CBB_add_asn1_oid_from_text(CBB *cbb, const char *text,
                                              size_t len);

// CBB_flush_asn1_set_of calls |CBB_flush| on |cbb| and then reorders the
// contents for a DER-encoded ASN.1 SET OF type. It returns one on success and
// zero on failure. DER canonicalizes SET OF contents by sorting
// lexicographically by encoding. Call this function when encoding a SET OF
// type in an order that is not already known to be canonical.
//
// Note a SET type has a slightly different ordering than a SET OF.
OPENSSL_EXPORT int CBB_flush_asn1_set_of(CBB *cbb);


// Unicode utilities.
//
// These functions consider noncharacters (see section 23.7 from Unicode 15.0.0)
// to be invalid code points and will treat them as an error condition.

// The following functions read one Unicode code point from |cbs| with the
// corresponding encoding and store it in |*out|. They return one on success and
// zero on error.
OPENSSL_EXPORT int CBS_get_utf8(CBS *cbs, uint32_t *out);
OPENSSL_EXPORT int CBS_get_latin1(CBS *cbs, uint32_t *out);
OPENSSL_EXPORT int CBS_get_ucs2_be(CBS *cbs, uint32_t *out);
OPENSSL_EXPORT int CBS_get_utf32_be(CBS *cbs, uint32_t *out);

// CBB_get_utf8_len returns the number of bytes needed to represent |u| in
// UTF-8.
OPENSSL_EXPORT size_t CBB_get_utf8_len(uint32_t u);

// The following functions encode |u| to |cbb| with the corresponding
// encoding. They return one on success and zero on error. Error conditions
// include |u| being an invalid code point, or |u| being unencodable in the
// specified encoding.
OPENSSL_EXPORT int CBB_add_utf8(CBB *cbb, uint32_t u);
OPENSSL_EXPORT int CBB_add_latin1(CBB *cbb, uint32_t u);
OPENSSL_EXPORT int CBB_add_ucs2_be(CBB *cbb, uint32_t u);
OPENSSL_EXPORT int CBB_add_utf32_be(CBB *cbb, uint32_t u);


#if defined(__cplusplus)
}  // extern C


#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

using ScopedCBB = internal::StackAllocated<CBB, void, CBB_zero, CBB_cleanup>;

BSSL_NAMESPACE_END

}  // extern C++
#endif

#endif

#endif  // OPENSSL_HEADER_BYTESTRING_H
