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
 */

#ifndef OPENSSL_HEADER_ASN1_H
#define OPENSSL_HEADER_ASN1_H

#include <openssl/base.h>

#include <time.h>

#include <openssl/bio.h>
#include <openssl/bn.h>
#include <openssl/stack.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Legacy ASN.1 library.
//
// This header is part of OpenSSL's ASN.1 implementation. It is retained for
// compatibility but should not be used by new code. The functions are difficult
// to use correctly, and have buggy or non-standard behaviors. They are thus
// particularly prone to behavior changes and API removals, as BoringSSL
// iterates on these issues.
//
// Use the new |CBS| and |CBB| library in <openssl/bytestring.h> instead.


// Tag constants.
//
// These constants are used in various APIs to specify ASN.1 types and tag
// components. See the specific API's documentation for details on which values
// are used and how.

// The following constants are tag classes.
#define V_ASN1_UNIVERSAL 0x00
#define V_ASN1_APPLICATION 0x40
#define V_ASN1_CONTEXT_SPECIFIC 0x80
#define V_ASN1_PRIVATE 0xc0

// V_ASN1_CONSTRUCTED indicates an element is constructed, rather than
// primitive.
#define V_ASN1_CONSTRUCTED 0x20

// V_ASN1_PRIMITIVE_TAG is the highest tag number which can be encoded in a
// single byte. Note this is unrelated to whether an element is constructed or
// primitive.
//
// TODO(davidben): Make this private.
#define V_ASN1_PRIMITIVE_TAG 0x1f

// V_ASN1_MAX_UNIVERSAL is the highest supported universal tag number. It is
// necessary to avoid ambiguity with |V_ASN1_NEG| and |MBSTRING_FLAG|.
//
// TODO(davidben): Make this private.
#define V_ASN1_MAX_UNIVERSAL 0xff

// V_ASN1_UNDEF is used in some APIs to indicate an ASN.1 element is omitted.
#define V_ASN1_UNDEF (-1)

// V_ASN1_OTHER is used in |ASN1_TYPE| to indicate a non-universal ASN.1 type.
#define V_ASN1_OTHER (-3)

// V_ASN1_ANY is used by the ASN.1 templates to indicate an ANY type.
#define V_ASN1_ANY (-4)

// The following constants are tag numbers for universal types.
#define V_ASN1_EOC 0
#define V_ASN1_BOOLEAN 1
#define V_ASN1_INTEGER 2
#define V_ASN1_BIT_STRING 3
#define V_ASN1_OCTET_STRING 4
#define V_ASN1_NULL 5
#define V_ASN1_OBJECT 6
#define V_ASN1_OBJECT_DESCRIPTOR 7
#define V_ASN1_EXTERNAL 8
#define V_ASN1_REAL 9
#define V_ASN1_ENUMERATED 10
#define V_ASN1_UTF8STRING 12
#define V_ASN1_SEQUENCE 16
#define V_ASN1_SET 17
#define V_ASN1_NUMERICSTRING 18
#define V_ASN1_PRINTABLESTRING 19
#define V_ASN1_T61STRING 20
#define V_ASN1_TELETEXSTRING 20
#define V_ASN1_VIDEOTEXSTRING 21
#define V_ASN1_IA5STRING 22
#define V_ASN1_UTCTIME 23
#define V_ASN1_GENERALIZEDTIME 24
#define V_ASN1_GRAPHICSTRING 25
#define V_ASN1_ISO64STRING 26
#define V_ASN1_VISIBLESTRING 26
#define V_ASN1_GENERALSTRING 27
#define V_ASN1_UNIVERSALSTRING 28
#define V_ASN1_BMPSTRING 30

// The following constants are used for |ASN1_STRING| values that represent
// negative INTEGER and ENUMERATED values. See |ASN1_STRING| for more details.
#define V_ASN1_NEG 0x100
#define V_ASN1_NEG_INTEGER (V_ASN1_INTEGER | V_ASN1_NEG)
#define V_ASN1_NEG_ENUMERATED (V_ASN1_ENUMERATED | V_ASN1_NEG)

// The following constants are bitmask representations of ASN.1 types.
#define B_ASN1_NUMERICSTRING 0x0001
#define B_ASN1_PRINTABLESTRING 0x0002
#define B_ASN1_T61STRING 0x0004
#define B_ASN1_TELETEXSTRING 0x0004
#define B_ASN1_VIDEOTEXSTRING 0x0008
#define B_ASN1_IA5STRING 0x0010
#define B_ASN1_GRAPHICSTRING 0x0020
#define B_ASN1_ISO64STRING 0x0040
#define B_ASN1_VISIBLESTRING 0x0040
#define B_ASN1_GENERALSTRING 0x0080
#define B_ASN1_UNIVERSALSTRING 0x0100
#define B_ASN1_OCTET_STRING 0x0200
#define B_ASN1_BIT_STRING 0x0400
#define B_ASN1_BMPSTRING 0x0800
#define B_ASN1_UNKNOWN 0x1000
#define B_ASN1_UTF8STRING 0x2000
#define B_ASN1_UTCTIME 0x4000
#define B_ASN1_GENERALIZEDTIME 0x8000
#define B_ASN1_SEQUENCE 0x10000

// ASN1_tag2bit converts |tag| from the tag number of a universal type to a
// corresponding |B_ASN1_*| constant, |B_ASN1_UNKNOWN|, or zero. If the
// |B_ASN1_*| constant above is defined, it will map the corresponding
// |V_ASN1_*| constant to it. Otherwise, whether it returns |B_ASN1_UNKNOWN| or
// zero is ill-defined and callers should not rely on it.
//
// TODO(https://crbug.com/boringssl/412): Figure out what |B_ASN1_UNNOWN| vs
// zero is meant to be. The main impact is what values go in |B_ASN1_PRINTABLE|.
// To that end, we must return zero on types that can't go in |ASN1_STRING|.
OPENSSL_EXPORT unsigned long ASN1_tag2bit(int tag);

// ASN1_tag2str returns a string representation of |tag|, interpret as a tag
// number for a universal type, or |V_ASN1_NEG_*|.
OPENSSL_EXPORT const char *ASN1_tag2str(int tag);


// API conventions.
//
// The following sample functions document the calling conventions used by
// legacy ASN.1 APIs.

#if 0  // Sample functions

// d2i_SAMPLE parses a structure from up to |len| bytes at |*inp|. On success,
// it advances |*inp| by the number of bytes read and returns a newly-allocated
// |SAMPLE| object containing the parsed structure. If |out| is non-NULL, it
// additionally frees the previous value at |*out| and updates |*out| to the
// result. If parsing or allocating the result fails, it returns NULL.
//
// This function does not reject trailing data in the input. This allows the
// caller to parse a sequence of concatenated structures. Callers parsing only
// one structure should check for trailing data by comparing the updated |*inp|
// with the end of the input.
//
// Note: If |out| and |*out| are both non-NULL, the object at |*out| is not
// updated in-place. Instead, it is freed, and the pointer is updated to the
// new object. This differs from OpenSSL, which behaves more like
// |d2i_SAMPLE_with_reuse|. Callers are recommended to set |out| to NULL and
// instead use the return value.
SAMPLE *d2i_SAMPLE(SAMPLE **out, const uint8_t **inp, long len);

// d2i_SAMPLE_with_reuse parses a structure from up to |len| bytes at |*inp|. On
// success, it advances |*inp| by the number of bytes read and returns a
// non-NULL pointer to an object containing the parsed structure. The object is
// determined from |out| as follows:
//
// If |out| is NULL, the function places the result in a newly-allocated
// |SAMPLE| object and returns it. This mode is recommended.
//
// If |out| is non-NULL, but |*out| is NULL, the function also places the result
// in a newly-allocated |SAMPLE| object. It sets |*out| to this object and also
// returns it.
//
// If |out| and |*out| are both non-NULL, the function updates the object at
// |*out| in-place with the result and returns |*out|.
//
// If any of the above fail, the function returns NULL.
//
// This function does not reject trailing data in the input. This allows the
// caller to parse a sequence of concatenated structures. Callers parsing only
// one structure should check for trailing data by comparing the updated |*inp|
// with the end of the input.
//
// WARNING: Callers should not rely on the in-place update mode. It often
// produces the wrong result or breaks the type's internal invariants. Future
// revisions of BoringSSL may standardize on the |d2i_SAMPLE| behavior.
SAMPLE *d2i_SAMPLE_with_reuse(SAMPLE **out, const uint8_t **inp, long len);

// i2d_SAMPLE marshals |in|. On error, it returns a negative value. On success,
// it returns the length of the result and outputs it via |outp| as follows:
//
// If |outp| is NULL, the function writes nothing. This mode can be used to size
// buffers.
//
// If |outp| is non-NULL but |*outp| is NULL, the function sets |*outp| to a
// newly-allocated buffer containing the result. The caller is responsible for
// releasing |*outp| with |OPENSSL_free|. This mode is recommended for most
// callers.
//
// If |outp| and |*outp| are non-NULL, the function writes the result to
// |*outp|, which must have enough space available, and advances |*outp| just
// past the output.
//
// WARNING: In the third mode, the function does not internally check output
// bounds. Failing to correctly size the buffer will result in a potentially
// exploitable memory error.
int i2d_SAMPLE(const SAMPLE *in, uint8_t **outp);

#endif  // Sample functions

// The following macros are used to retrieve the function pointer of the
// |d2i| or |i2d| ASN1 functions of |type|.
//
// NOTE: |D2I_OF| and |I2D_OF_const| are not implemented.
#define I2D_OF(type) int (*)(type *, unsigned char **)

// CHECKED_I2D_OF casts a given pointer to i2d_of_void* and statically checks
// that it was a pointer to |type|'s |i2d| function.
#define CHECKED_I2D_OF(type, i2d) ((i2d_of_void *)(1 ? i2d : ((I2D_OF(type))0)))

// The following typedefs are sometimes used for pointers to functions like
// |d2i_SAMPLE| and |i2d_SAMPLE|. Note, however, that these act on |void*|.
// Calling a function with a different pointer type is undefined in C, so this
// is only valid with a wrapper.
typedef void *d2i_of_void(void **, const unsigned char **, long);
typedef int i2d_of_void(const void *, unsigned char **);


// ASN.1 types.
//
// An |ASN1_ITEM| represents an ASN.1 type and allows working with ASN.1 types
// generically.
//
// |ASN1_ITEM|s use a different namespace from C types and are accessed via
// |ASN1_ITEM_*| macros. So, for example, |ASN1_OCTET_STRING| is both a C type
// and the name of an |ASN1_ITEM|, referenced as
// |ASN1_ITEM_rptr(ASN1_OCTET_STRING)|.
//
// Each |ASN1_ITEM| has a corresponding C type, typically with the same name,
// which represents values in the ASN.1 type. This type is either a pointer type
// or |ASN1_BOOLEAN|. When it is a pointer, NULL pointers represent omitted
// values. For example, an OCTET STRING value is declared with the C type
// |ASN1_OCTET_STRING*| and uses the |ASN1_ITEM| named |ASN1_OCTET_STRING|. An
// OPTIONAL OCTET STRING uses the same C type and represents an omitted value
// with a NULL pointer. |ASN1_BOOLEAN| is described in a later section.

// DECLARE_ASN1_ITEM declares an |ASN1_ITEM| with name |name|. The |ASN1_ITEM|
// may be referenced with |ASN1_ITEM_rptr|. Uses of this macro should document
// the corresponding ASN.1 and C types.
#define DECLARE_ASN1_ITEM(name) extern OPENSSL_EXPORT const ASN1_ITEM name##_it;

// ASN1_ITEM_rptr returns the |const ASN1_ITEM *| named |name|.
#define ASN1_ITEM_rptr(name) (&(name##_it))

// ASN1_ITEM_EXP is an abstraction for referencing an |ASN1_ITEM| in a
// constant-initialized structure, such as a method table. It exists because, on
// some OpenSSL platforms, |ASN1_ITEM| references are indirected through
// functions. Structures reference the |ASN1_ITEM| by declaring a field like
// |ASN1_ITEM_EXP *item| and initializing it with |ASN1_ITEM_ref|.
typedef const ASN1_ITEM ASN1_ITEM_EXP;

// ASN1_ITEM_ref returns an |ASN1_ITEM_EXP*| for the |ASN1_ITEM| named |name|.
#define ASN1_ITEM_ref(name) (&(name##_it))

// ASN1_ITEM_ptr converts |iptr|, which must be an |ASN1_ITEM_EXP*| to a
// |const ASN1_ITEM*|.
#define ASN1_ITEM_ptr(iptr) (iptr)

// ASN1_VALUE_st (aka |ASN1_VALUE|) is an opaque type used as a placeholder for
// the C type corresponding to an |ASN1_ITEM|.
typedef struct ASN1_VALUE_st ASN1_VALUE;

// ASN1_parse performs an ASN.1 dump of the contents pointed to by |pp| of length |len|,
// and writes it to |bp|. Returns 1 on success, or 0 on failure.
OPENSSL_EXPORT int ASN1_parse(BIO *bp, const unsigned char *pp, long len, int indent);

// ASN1_item_new allocates a new value of the C type corresponding to |it|, or
// NULL on error. On success, the caller must release the value with
// |ASN1_item_free|, or the corresponding C type's free function, when done. The
// new value will initialize fields of the value to some default state, such as
// an empty string. Note, however, that this default state sometimes omits
// required values, such as with CHOICE types.
//
// This function may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
//
// WARNING: Casting the result of this function to the wrong type is a
// potentially exploitable memory error. Callers must ensure the value is used
// consistently with |it|. Prefer using type-specific functions such as
// |ASN1_OCTET_STRING_new|.
OPENSSL_EXPORT ASN1_VALUE *ASN1_item_new(const ASN1_ITEM *it);

// ASN1_item_free releases memory associated with |val|, which must be an object
// of the C type corresponding to |it|.
//
// This function may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
//
// WARNING: Passing a pointer of the wrong type into this function is a
// potentially exploitable memory error. Callers must ensure |val| is consistent
// with |it|. Prefer using type-specific functions such as
// |ASN1_OCTET_STRING_free|.
OPENSSL_EXPORT void ASN1_item_free(ASN1_VALUE *val, const ASN1_ITEM *it);

// ASN1_item_d2i parses the ASN.1 type |it| from up to |len| bytes at |*inp|.
// It behaves like |d2i_SAMPLE_with_reuse|, except that |out| and the return
// value are cast to |ASN1_VALUE| pointers.
//
// TODO(https://crbug.com/boringssl/444): C strict aliasing forbids type-punning
// |T*| and |ASN1_VALUE*| the way this function signature does. When that bug is
// resolved, we will need to pick which type |*out| is (probably |T*|). Do not
// use a non-NULL |out| to avoid ending up on the wrong side of this question.
//
// This function may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
//
// WARNING: Casting the result of this function to the wrong type, or passing a
// pointer of the wrong type into this function, are potentially exploitable
// memory errors. Callers must ensure |out| is consistent with |it|. Prefer
// using type-specific functions such as |d2i_ASN1_OCTET_STRING|.
OPENSSL_EXPORT ASN1_VALUE *ASN1_item_d2i(ASN1_VALUE **out,
                                         const unsigned char **inp, long len,
                                         const ASN1_ITEM *it);

// ASN1_item_i2d marshals |val| as the ASN.1 type associated with |it|, as
// described in |i2d_SAMPLE|.
//
// This function may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
//
// WARNING: Passing a pointer of the wrong type into this function is a
// potentially exploitable memory error. Callers must ensure |val| is consistent
// with |it|. Prefer using type-specific functions such as
// |i2d_ASN1_OCTET_STRING|.
OPENSSL_EXPORT int ASN1_item_i2d(ASN1_VALUE *val, unsigned char **outp,
                                 const ASN1_ITEM *it);

// ASN1_dup returns a newly-allocated copy of |x| by re-encoding with |i2d| and
// |d2i|. |i2d| and |d2i| must be the corresponding type functions of |x|. NULL
// is returned on error.
//
// WARNING: DO NOT USE. Casting the result of this function to the wrong type,
// or passing a pointer of the wrong type into this function, are potentially
// exploitable memory errors. Prefer directly calling |i2d| and |d2i| or other
// type-specific functions.
OPENSSL_EXPORT void *ASN1_dup(i2d_of_void *i2d, d2i_of_void *d2i, void *x);

// ASN1_item_dup returns a newly-allocated copy of |x|, or NULL on error. |x|
// must be an object of |it|'s C type.
//
// This function may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
//
// WARNING: Casting the result of this function to the wrong type, or passing a
// pointer of the wrong type into this function, are potentially exploitable
// memory errors. Prefer using type-specific functions such as
// |ASN1_STRING_dup|.
OPENSSL_EXPORT void *ASN1_item_dup(const ASN1_ITEM *it, void *x);

// The following functions behave like |ASN1_item_d2i| but read from |in|
// instead. |out| is the same parameter as in |ASN1_item_d2i|, but written with
// |void*| instead. The return values similarly match.
//
// These functions may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
//
// WARNING: These functions do not bound how much data is read from |in|.
// Parsing an untrusted input could consume unbounded memory.
OPENSSL_EXPORT void *ASN1_item_d2i_fp(const ASN1_ITEM *it, FILE *in, void *out);
OPENSSL_EXPORT void *ASN1_item_d2i_bio(const ASN1_ITEM *it, BIO *in, void *out);

// The following functions behave like |ASN1_item_i2d| but write to |out|
// instead. |in| is the same parameter as in |ASN1_item_i2d|, but written with
// |void*| instead.
//
// These functions may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
OPENSSL_EXPORT int ASN1_item_i2d_fp(const ASN1_ITEM *it, FILE *out, void *in);
OPENSSL_EXPORT int ASN1_item_i2d_bio(const ASN1_ITEM *it, BIO *out, void *in);

// ASN1_i2d_bio writes the result to |out| like the functions above, but parses
// |in| with |i2d|.
//
// WARNING: Prefer using type-specific functions. |i2d| is only valid with a
// wrapper (see |d2i_of_void|).
OPENSSL_EXPORT int ASN1_i2d_bio(i2d_of_void *i2d, BIO *out, void *in);

// ASN1_i2d_bio_of statically checks |i2d| and |in| for |type| before casting
// and calling |ASN1_i2d_bio|.
//
// DO NOT USE: This is only maintained for compatibility purposes. Calling a
// function with a different pointer type is undefined behavior in C (see
// |d2i_of_void|).
// This macro forces the user to directly call the |i2d| ASN.1 function of
// |type|. |i2d| functions of |type| are defined with parameters of |type *|,
// but |i2d_of_void| expects the signature of a |const void *|. This inherently
// forces the user to use undefined C behavior and will cause failures when
// running against undefined behavior sanitizers in clang.
#define ASN1_i2d_bio_of(type, i2d, out, in) \
  (ASN1_i2d_bio(CHECKED_I2D_OF(type, i2d), out, CHECKED_PTR_OF(type, in)))

// ASN1_item_unpack parses |oct|'s contents as |it|'s ASN.1 type. It returns a
// newly-allocated instance of |it|'s C type on success, or NULL on error.
//
// This function may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
//
// WARNING: Casting the result of this function to the wrong type is a
// potentially exploitable memory error. Callers must ensure the value is used
// consistently with |it|.
OPENSSL_EXPORT void *ASN1_item_unpack(const ASN1_STRING *oct,
                                      const ASN1_ITEM *it);

// ASN1_item_pack marshals |obj| as |it|'s ASN.1 type. If |out| is NULL, it
// returns a newly-allocated |ASN1_STRING| with the result, or NULL on error.
// If |out| is non-NULL, but |*out| is NULL, it does the same but additionally
// sets |*out| to the result. If both |out| and |*out| are non-NULL, it writes
// the result to |*out| and returns |*out| on success or NULL on error.
//
// This function may not be used with |ASN1_ITEM|s whose C type is
// |ASN1_BOOLEAN|.
//
// WARNING: Passing a pointer of the wrong type into this function is a
// potentially exploitable memory error. Callers must ensure |val| is consistent
// with |it|.
OPENSSL_EXPORT ASN1_STRING *ASN1_item_pack(void *obj, const ASN1_ITEM *it,
                                           ASN1_STRING **out);


// Booleans.
//
// This library represents ASN.1 BOOLEAN values with |ASN1_BOOLEAN|, which is an
// integer type. FALSE is zero, TRUE is 0xff, and an omitted OPTIONAL BOOLEAN is
// -1.

// ASN1_BOOLEAN_FALSE is FALSE as an |ASN1_BOOLEAN|.
#define ASN1_BOOLEAN_FALSE 0

// ASN1_BOOLEAN_TRUE is TRUE as an |ASN1_BOOLEAN|. Some code incorrectly uses
// 1, so prefer |b != ASN1_BOOLEAN_FALSE| over |b == ASN1_BOOLEAN_TRUE|.
#define ASN1_BOOLEAN_TRUE 0xff

// ASN1_BOOLEAN_NONE, in contexts where the |ASN1_BOOLEAN| represents an
// OPTIONAL BOOLEAN, is an omitted value. Using this value in other contexts is
// undefined and may be misinterpreted as TRUE.
#define ASN1_BOOLEAN_NONE (-1)

// d2i_ASN1_BOOLEAN parses a DER-encoded ASN.1 BOOLEAN from up to |len| bytes at
// |*inp|. On success, it advances |*inp| by the number of bytes read and
// returns the result. If |out| is non-NULL, it additionally writes the result
// to |*out|. On error, it returns |ASN1_BOOLEAN_NONE|.
//
// This function does not reject trailing data in the input. This allows the
// caller to parse a sequence of concatenated structures. Callers parsing only
// one structure should check for trailing data by comparing the updated |*inp|
// with the end of the input.
//
// WARNING: This function's is slightly different from other |d2i_*| functions
// because |ASN1_BOOLEAN| is not a pointer type.
OPENSSL_EXPORT ASN1_BOOLEAN d2i_ASN1_BOOLEAN(ASN1_BOOLEAN *out,
                                             const unsigned char **inp,
                                             long len);

// i2d_ASN1_BOOLEAN marshals |a| as a DER-encoded ASN.1 BOOLEAN, as described in
// |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_BOOLEAN(ASN1_BOOLEAN a, unsigned char **outp);

// The following |ASN1_ITEM|s have ASN.1 type BOOLEAN and C type |ASN1_BOOLEAN|.
// |ASN1_TBOOLEAN| and |ASN1_FBOOLEAN| must be marked OPTIONAL. When omitted,
// they are parsed as TRUE and FALSE, respectively, rather than
// |ASN1_BOOLEAN_NONE|.
DECLARE_ASN1_ITEM(ASN1_BOOLEAN)
DECLARE_ASN1_ITEM(ASN1_TBOOLEAN)
DECLARE_ASN1_ITEM(ASN1_FBOOLEAN)


// Strings.
//
// ASN.1 contains a myriad of string types, as well as types that contain data
// that may be encoded into a string. This library uses a single type,
// |ASN1_STRING|, to represent most values.

// An asn1_string_st (aka |ASN1_STRING|) represents a value of a string-like
// ASN.1 type. It contains a type field, and a byte string data field with a
// type-specific representation.
//
// When representing a string value, the type field is one of
// |V_ASN1_OCTET_STRING|, |V_ASN1_UTF8STRING|, |V_ASN1_NUMERICSTRING|,
// |V_ASN1_PRINTABLESTRING|, |V_ASN1_T61STRING|, |V_ASN1_VIDEOTEXSTRING|,
// |V_ASN1_IA5STRING|, |V_ASN1_GRAPHICSTRING|, |V_ASN1_ISO64STRING|,
// |V_ASN1_VISIBLESTRING|, |V_ASN1_GENERALSTRING|, |V_ASN1_UNIVERSALSTRING|, or
// |V_ASN1_BMPSTRING|. The data contains the byte representation of of the
// string.
//
// When representing a BIT STRING value, the type field is |V_ASN1_BIT_STRING|.
// See bit string documentation below for how the data and flags are used.
//
// When representing an INTEGER or ENUMERATED value, the type field is one of
// |V_ASN1_INTEGER|, |V_ASN1_NEG_INTEGER|, |V_ASN1_ENUMERATED|, or
// |V_ASN1_NEG_ENUMERATED|. See integer documentation below for details.
//
// When representing a GeneralizedTime or UTCTime value, the type field is
// |V_ASN1_GENERALIZEDTIME| or |V_ASN1_UTCTIME|, respectively. The data contains
// the DER encoding of the value. For example, the UNIX epoch would be
// "19700101000000Z" for a GeneralizedTime and "700101000000Z" for a UTCTime.
//
// |ASN1_STRING|, when stored in an |ASN1_TYPE|, may also represent an element
// with tag not directly supported by this library. See |ASN1_TYPE| for details.
//
// |ASN1_STRING| additionally has the following typedefs: |ASN1_BIT_STRING|,
// |ASN1_BMPSTRING|, |ASN1_ENUMERATED|, |ASN1_GENERALIZEDTIME|,
// |ASN1_GENERALSTRING|, |ASN1_IA5STRING|, |ASN1_INTEGER|, |ASN1_OCTET_STRING|,
// |ASN1_PRINTABLESTRING|, |ASN1_T61STRING|, |ASN1_TIME|,
// |ASN1_UNIVERSALSTRING|, |ASN1_UTCTIME|, |ASN1_UTF8STRING|, and
// |ASN1_VISIBLESTRING|. Other than |ASN1_TIME|, these correspond to universal
// ASN.1 types. |ASN1_TIME| represents a CHOICE of UTCTime and GeneralizedTime,
// with a cutoff of 2049, as used in Section 4.1.2.5 of RFC 5280.
//
// For clarity, callers are encouraged to use the appropriate typedef when
// available. They are the same type as |ASN1_STRING|, so a caller may freely
// pass them into functions expecting |ASN1_STRING|, such as
// |ASN1_STRING_length|.
//
// If a function returns an |ASN1_STRING| where the typedef or ASN.1 structure
// implies constraints on the type field, callers may assume that the type field
// is correct. However, if a function takes an |ASN1_STRING| as input, callers
// must ensure the type field matches. These invariants are not captured by the
// C type system and may not be checked at runtime. For example, callers may
// assume the output of |X509_get0_serialNumber| has type |V_ASN1_INTEGER| or
// |V_ASN1_NEG_INTEGER|. Callers must not pass a string of type
// |V_ASN1_OCTET_STRING| to |X509_set_serialNumber|. Doing so may break
// invariants on the |X509| object and break the |X509_get0_serialNumber|
// invariant.
//
// TODO(https://crbug.com/boringssl/445): This is very unfriendly. Getting the
// type field wrong should not cause memory errors, but it may do strange
// things. We should add runtime checks to anything that consumes |ASN1_STRING|s
// from the caller.
struct asn1_string_st {
  int length;
  int type;
  unsigned char *data;
  long flags;
};

// ASN1_STRING_FLAG_BITS_LEFT indicates, in a BIT STRING |ASN1_STRING|, that
// flags & 0x7 contains the number of padding bits added to the BIT STRING
// value. When not set, all trailing zero bits in the last byte are implicitly
// treated as padding. This behavior is deprecated and should not be used.
#define ASN1_STRING_FLAG_BITS_LEFT 0x08

// ASN1_STRING_type_new returns a newly-allocated empty |ASN1_STRING| object of
// type |type|, or NULL on error.
OPENSSL_EXPORT ASN1_STRING *ASN1_STRING_type_new(int type);

// ASN1_STRING_new returns a newly-allocated empty |ASN1_STRING| object with an
// arbitrary type. Prefer one of the type-specific constructors, such as
// |ASN1_OCTET_STRING_new|, or |ASN1_STRING_type_new|.
OPENSSL_EXPORT ASN1_STRING *ASN1_STRING_new(void);

// ASN1_STRING_free releases memory associated with |str|.
OPENSSL_EXPORT void ASN1_STRING_free(ASN1_STRING *str);

// ASN1_STRING_clear_free releases memory associated with |str|.
OPENSSL_EXPORT void ASN1_STRING_clear_free(ASN1_STRING *str);

// ASN1_STRING_copy sets |dst| to a copy of |str|. It returns one on success and
// zero on error.
OPENSSL_EXPORT int ASN1_STRING_copy(ASN1_STRING *dst, const ASN1_STRING *str);

// ASN1_STRING_dup returns a newly-allocated copy of |str|, or NULL on error.
OPENSSL_EXPORT ASN1_STRING *ASN1_STRING_dup(const ASN1_STRING *str);

// ASN1_STRING_type returns the type of |str|. This value will be one of the
// |V_ASN1_*| constants.
OPENSSL_EXPORT int ASN1_STRING_type(const ASN1_STRING *str);

// ASN1_STRING_get0_data returns a pointer to |str|'s contents. Callers should
// use |ASN1_STRING_length| to determine the length of the string. The string
// may have embedded NUL bytes and may not be NUL-terminated.
OPENSSL_EXPORT const unsigned char *ASN1_STRING_get0_data(
    const ASN1_STRING *str);

// ASN1_STRING_data returns a mutable pointer to |str|'s contents. Callers
// should use |ASN1_STRING_length| to determine the length of the string. The
// string may have embedded NUL bytes and may not be NUL-terminated.
//
// Prefer |ASN1_STRING_get0_data|.
OPENSSL_EXPORT unsigned char *ASN1_STRING_data(ASN1_STRING *str);

// ASN1_STRING_length returns the length of |str|, in bytes.
OPENSSL_EXPORT int ASN1_STRING_length(const ASN1_STRING *str);

// ASN1_STRING_cmp compares |a| and |b|'s type and contents. It returns an
// integer equal to, less than, or greater than zero if |a| is equal to, less
// than, or greater than |b|, respectively. This function compares by length,
// then data, then type. Note the data compared is the |ASN1_STRING| internal
// representation and the type order is arbitrary. While this comparison is
// suitable for sorting, callers should not rely on the exact order when |a|
// and |b| are different types.
//
// Note that, if |a| and |b| are INTEGERs, this comparison does not order the
// values numerically. For a numerical comparison, use |ASN1_INTEGER_cmp|.
OPENSSL_EXPORT int ASN1_STRING_cmp(const ASN1_STRING *a, const ASN1_STRING *b);

// ASN1_STRING_set sets the contents of |str| to a copy of |len| bytes from
// |data|. It returns one on success and zero on error. If |data| is NULL, it
// updates the length and allocates the buffer as needed, but does not
// initialize the contents.
OPENSSL_EXPORT int ASN1_STRING_set(ASN1_STRING *str, const void *data,
                                   ossl_ssize_t len);

// ASN1_STRING_set0 sets the contents of |str| to |len| bytes from |data|. It
// takes ownership of |data|, which must have been allocated with
// |OPENSSL_malloc|.
OPENSSL_EXPORT void ASN1_STRING_set0(ASN1_STRING *str, void *data, int len);

// The following functions call |ASN1_STRING_type_new| with the corresponding
// |V_ASN1_*| constant.
OPENSSL_EXPORT ASN1_BMPSTRING *ASN1_BMPSTRING_new(void);
OPENSSL_EXPORT ASN1_GENERALSTRING *ASN1_GENERALSTRING_new(void);
OPENSSL_EXPORT ASN1_IA5STRING *ASN1_IA5STRING_new(void);
OPENSSL_EXPORT ASN1_OCTET_STRING *ASN1_OCTET_STRING_new(void);
OPENSSL_EXPORT ASN1_PRINTABLESTRING *ASN1_PRINTABLESTRING_new(void);
OPENSSL_EXPORT ASN1_T61STRING *ASN1_T61STRING_new(void);
OPENSSL_EXPORT ASN1_UNIVERSALSTRING *ASN1_UNIVERSALSTRING_new(void);
OPENSSL_EXPORT ASN1_UTF8STRING *ASN1_UTF8STRING_new(void);
OPENSSL_EXPORT ASN1_VISIBLESTRING *ASN1_VISIBLESTRING_new(void);

// The following functions call |ASN1_STRING_free|.
OPENSSL_EXPORT void ASN1_BMPSTRING_free(ASN1_BMPSTRING *str);
OPENSSL_EXPORT void ASN1_GENERALSTRING_free(ASN1_GENERALSTRING *str);
OPENSSL_EXPORT void ASN1_IA5STRING_free(ASN1_IA5STRING *str);
OPENSSL_EXPORT void ASN1_OCTET_STRING_free(ASN1_OCTET_STRING *str);
OPENSSL_EXPORT void ASN1_PRINTABLESTRING_free(ASN1_PRINTABLESTRING *str);
OPENSSL_EXPORT void ASN1_T61STRING_free(ASN1_T61STRING *str);
OPENSSL_EXPORT void ASN1_UNIVERSALSTRING_free(ASN1_UNIVERSALSTRING *str);
OPENSSL_EXPORT void ASN1_UTF8STRING_free(ASN1_UTF8STRING *str);
OPENSSL_EXPORT void ASN1_VISIBLESTRING_free(ASN1_VISIBLESTRING *str);

// The following functions parse up to |len| bytes from |*inp| as a
// DER-encoded ASN.1 value of the corresponding type, as described in
// |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_BMPSTRING *d2i_ASN1_BMPSTRING(ASN1_BMPSTRING **out,
                                                  const uint8_t **inp,
                                                  long len);
OPENSSL_EXPORT ASN1_GENERALSTRING *d2i_ASN1_GENERALSTRING(
    ASN1_GENERALSTRING **out, const uint8_t **inp, long len);
OPENSSL_EXPORT ASN1_IA5STRING *d2i_ASN1_IA5STRING(ASN1_IA5STRING **out,
                                                  const uint8_t **inp,
                                                  long len);
OPENSSL_EXPORT ASN1_OCTET_STRING *d2i_ASN1_OCTET_STRING(ASN1_OCTET_STRING **out,
                                                        const uint8_t **inp,
                                                        long len);
OPENSSL_EXPORT ASN1_PRINTABLESTRING *d2i_ASN1_PRINTABLESTRING(
    ASN1_PRINTABLESTRING **out, const uint8_t **inp, long len);
OPENSSL_EXPORT ASN1_T61STRING *d2i_ASN1_T61STRING(ASN1_T61STRING **out,
                                                  const uint8_t **inp,
                                                  long len);
OPENSSL_EXPORT ASN1_UNIVERSALSTRING *d2i_ASN1_UNIVERSALSTRING(
    ASN1_UNIVERSALSTRING **out, const uint8_t **inp, long len);
OPENSSL_EXPORT ASN1_UTF8STRING *d2i_ASN1_UTF8STRING(ASN1_UTF8STRING **out,
                                                    const uint8_t **inp,
                                                    long len);
OPENSSL_EXPORT ASN1_VISIBLESTRING *d2i_ASN1_VISIBLESTRING(
    ASN1_VISIBLESTRING **out, const uint8_t **inp, long len);

// The following functions marshal |in| as a DER-encoded ASN.1 value of the
// corresponding type, as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_BMPSTRING(const ASN1_BMPSTRING *in, uint8_t **outp);
OPENSSL_EXPORT int i2d_ASN1_GENERALSTRING(const ASN1_GENERALSTRING *in,
                                          uint8_t **outp);
OPENSSL_EXPORT int i2d_ASN1_IA5STRING(const ASN1_IA5STRING *in, uint8_t **outp);
OPENSSL_EXPORT int i2d_ASN1_OCTET_STRING(const ASN1_OCTET_STRING *in,
                                         uint8_t **outp);
OPENSSL_EXPORT int i2d_ASN1_PRINTABLESTRING(const ASN1_PRINTABLESTRING *in,
                                            uint8_t **outp);
OPENSSL_EXPORT int i2d_ASN1_T61STRING(const ASN1_T61STRING *in, uint8_t **outp);
OPENSSL_EXPORT int i2d_ASN1_UNIVERSALSTRING(const ASN1_UNIVERSALSTRING *in,
                                            uint8_t **outp);
OPENSSL_EXPORT int i2d_ASN1_UTF8STRING(const ASN1_UTF8STRING *in,
                                       uint8_t **outp);
OPENSSL_EXPORT int i2d_ASN1_VISIBLESTRING(const ASN1_VISIBLESTRING *in,
                                          uint8_t **outp);

// The following |ASN1_ITEM|s have the ASN.1 type referred to in their name and
// C type |ASN1_STRING*|. The C type may also be written as the corresponding
// typedef.
DECLARE_ASN1_ITEM(ASN1_BMPSTRING)
DECLARE_ASN1_ITEM(ASN1_GENERALSTRING)
DECLARE_ASN1_ITEM(ASN1_IA5STRING)
DECLARE_ASN1_ITEM(ASN1_OCTET_STRING)
DECLARE_ASN1_ITEM(ASN1_PRINTABLESTRING)
DECLARE_ASN1_ITEM(ASN1_T61STRING)
DECLARE_ASN1_ITEM(ASN1_UNIVERSALSTRING)
DECLARE_ASN1_ITEM(ASN1_UTF8STRING)
DECLARE_ASN1_ITEM(ASN1_VISIBLESTRING)

// ASN1_OCTET_STRING_dup calls |ASN1_STRING_dup|.
OPENSSL_EXPORT ASN1_OCTET_STRING *ASN1_OCTET_STRING_dup(
    const ASN1_OCTET_STRING *a);

// ASN1_OCTET_STRING_cmp calls |ASN1_STRING_cmp|.
OPENSSL_EXPORT int ASN1_OCTET_STRING_cmp(const ASN1_OCTET_STRING *a,
                                         const ASN1_OCTET_STRING *b);

// ASN1_OCTET_STRING_set calls |ASN1_STRING_set|.
OPENSSL_EXPORT int ASN1_OCTET_STRING_set(ASN1_OCTET_STRING *str,
                                         const unsigned char *data, int len);

// ASN1_STRING_to_UTF8 converts |in| to UTF-8. On success, sets |*out| to a
// newly-allocated buffer containing the resulting string and returns the length
// of the string. The caller must call |OPENSSL_free| to release |*out| when
// done. On error, it returns a negative number.
OPENSSL_EXPORT int ASN1_STRING_to_UTF8(unsigned char **out,
                                       const ASN1_STRING *in);

// The following formats define encodings for use with functions like
// |ASN1_mbstring_copy|. Note |MBSTRING_ASC| refers to Latin-1, not ASCII.
#define MBSTRING_FLAG 0x1000
#define MBSTRING_UTF8 (MBSTRING_FLAG)
#define MBSTRING_ASC (MBSTRING_FLAG | 1)
#define MBSTRING_BMP (MBSTRING_FLAG | 2)
#define MBSTRING_UNIV (MBSTRING_FLAG | 4)

// DIRSTRING_TYPE contains the valid string types in an X.509 DirectoryString.
#define DIRSTRING_TYPE                                            \
  (B_ASN1_PRINTABLESTRING | B_ASN1_T61STRING | B_ASN1_BMPSTRING | \
   B_ASN1_UTF8STRING)

// PKCS9STRING_TYPE contains the valid string types in a PKCS9String.
#define PKCS9STRING_TYPE (DIRSTRING_TYPE | B_ASN1_IA5STRING)

// ASN1_mbstring_copy converts |len| bytes from |in| to an ASN.1 string. If
// |len| is -1, |in| must be NUL-terminated and the length is determined by
// |strlen|. |in| is decoded according to |inform|, which must be one of
// |MBSTRING_*|. |mask| determines the set of valid output types and is a
// bitmask containing a subset of |B_ASN1_PRINTABLESTRING|, |B_ASN1_IA5STRING|,
// |B_ASN1_T61STRING|, |B_ASN1_BMPSTRING|, |B_ASN1_UNIVERSALSTRING|, and
// |B_ASN1_UTF8STRING|, in that preference order. This function chooses the
// first output type in |mask| which can represent |in|. It interprets T61String
// as Latin-1, rather than T.61.
//
// If |mask| is zero, |DIRSTRING_TYPE| is used by default.
//
// On success, this function returns the |V_ASN1_*| constant corresponding to
// the selected output type and, if |out| and |*out| are both non-NULL, updates
// the object at |*out| with the result. If |out| is non-NULL and |*out| is
// NULL, it instead sets |*out| to a newly-allocated |ASN1_STRING| containing
// the result. If |out| is NULL, it returns the selected output type without
// constructing an |ASN1_STRING|. On error, this function returns -1.
OPENSSL_EXPORT int ASN1_mbstring_copy(ASN1_STRING **out, const uint8_t *in,
                                      ossl_ssize_t len, int inform,
                                      unsigned long mask);

// ASN1_mbstring_ncopy behaves like |ASN1_mbstring_copy| but returns an error if
// the input is less than |minsize| or greater than |maxsize| codepoints long. A
// |maxsize| value of zero is ignored. Note the sizes are measured in
// codepoints, not output bytes.
OPENSSL_EXPORT int ASN1_mbstring_ncopy(ASN1_STRING **out, const uint8_t *in,
                                       ossl_ssize_t len, int inform,
                                       unsigned long mask, ossl_ssize_t minsize,
                                       ossl_ssize_t maxsize);

// ASN1_STRING_set_by_NID behaves like |ASN1_mbstring_ncopy|, but determines
// |mask|, |minsize|, and |maxsize| based on |nid|. When |nid| is a recognized
// X.509 attribute type, it will pick a suitable ASN.1 string type and bounds.
// For most attribute types, it preferentially chooses UTF8String. If |nid| is
// unrecognized, it uses UTF8String by default.
//
// Slightly unlike |ASN1_mbstring_ncopy|, this function interprets |out| and
// returns its result as follows: If |out| is NULL, it returns a newly-allocated
// |ASN1_STRING| containing the result. If |out| is non-NULL and
// |*out| is NULL, it additionally sets |*out| to the result. If both |out| and
// |*out| are non-NULL, it instead updates the object at |*out| and returns
// |*out|. In all cases, it returns NULL on error.
//
// This function supports the following NIDs: |NID_countryName|,
// |NID_dnQualifier|, |NID_domainComponent|, |NID_friendlyName|,
// |NID_givenName|, |NID_initials|, |NID_localityName|, |NID_ms_csp_name|,
// |NID_name|, |NID_organizationalUnitName|, |NID_organizationName|,
// |NID_pkcs9_challengePassword|, |NID_pkcs9_emailAddress|,
// |NID_pkcs9_unstructuredAddress|, |NID_pkcs9_unstructuredName|,
// |NID_serialNumber|, |NID_stateOrProvinceName|, and |NID_surname|. Additional
// NIDs may be registered with |ASN1_STRING_set_by_NID|, but it is recommended
// to call |ASN1_mbstring_ncopy| directly instead.
OPENSSL_EXPORT ASN1_STRING *ASN1_STRING_set_by_NID(ASN1_STRING **out,
                                                   const unsigned char *in,
                                                   ossl_ssize_t len, int inform,
                                                   int nid);

// STABLE_NO_MASK causes |ASN1_STRING_TABLE_add| to allow types other than
// UTF8String.
#define STABLE_NO_MASK 0x02

// ASN1_STRING_TABLE_add registers the corresponding parameters with |nid|, for
// use with |ASN1_STRING_set_by_NID|. It returns one on success and zero on
// error. It is an error to call this function if |nid| is a built-in NID, or
// was already registered by a previous call.
//
// WARNING: This function affects global state in the library. If two libraries
// in the same address space register information for the same OID, one call
// will fail. Prefer directly passing the desired parametrs to
// |ASN1_mbstring_copy| or |ASN1_mbstring_ncopy| instead.
OPENSSL_EXPORT int ASN1_STRING_TABLE_add(int nid, long minsize, long maxsize,
                                         unsigned long mask,
                                         unsigned long flags);


// Multi-strings.
//
// A multi-string, or "MSTRING", is an |ASN1_STRING| that represents a CHOICE of
// several string or string-like types, such as X.509's DirectoryString. The
// |ASN1_STRING|'s type field determines which type is used.
//
// Multi-string types are associated with a bitmask, using the |B_ASN1_*|
// constants, which defines which types are valid.

// B_ASN1_DIRECTORYSTRING is a bitmask of types allowed in an X.509
// DirectoryString (RFC 5280).
#define B_ASN1_DIRECTORYSTRING                                        \
  (B_ASN1_PRINTABLESTRING | B_ASN1_TELETEXSTRING | B_ASN1_BMPSTRING | \
   B_ASN1_UNIVERSALSTRING | B_ASN1_UTF8STRING)

// DIRECTORYSTRING_new returns a newly-allocated |ASN1_STRING| with type -1, or
// NULL on error. The resulting |ASN1_STRING| is not a valid X.509
// DirectoryString until initialized with a value.
OPENSSL_EXPORT ASN1_STRING *DIRECTORYSTRING_new(void);

// DIRECTORYSTRING_free calls |ASN1_STRING_free|.
OPENSSL_EXPORT void DIRECTORYSTRING_free(ASN1_STRING *str);

// d2i_DIRECTORYSTRING parses up to |len| bytes from |*inp| as a DER-encoded
// X.509 DirectoryString (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
//
// TODO(https://crbug.com/boringssl/449): DirectoryString's non-empty string
// requirement is not currently enforced.
OPENSSL_EXPORT ASN1_STRING *d2i_DIRECTORYSTRING(ASN1_STRING **out,
                                                const uint8_t **inp, long len);

// i2d_DIRECTORYSTRING marshals |in| as a DER-encoded X.509 DirectoryString (RFC
// 5280), as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_DIRECTORYSTRING(const ASN1_STRING *in, uint8_t **outp);

// DIRECTORYSTRING is an |ASN1_ITEM| whose ASN.1 type is X.509 DirectoryString
// (RFC 5280) and C type is |ASN1_STRING*|.
DECLARE_ASN1_ITEM(DIRECTORYSTRING)

// B_ASN1_DISPLAYTEXT is a bitmask of types allowed in an X.509 DisplayText (RFC
// 5280).
#define B_ASN1_DISPLAYTEXT                                      \
  (B_ASN1_IA5STRING | B_ASN1_VISIBLESTRING | B_ASN1_BMPSTRING | \
   B_ASN1_UTF8STRING)

// DISPLAYTEXT_new returns a newly-allocated |ASN1_STRING| with type -1, or NULL
// on error. The resulting |ASN1_STRING| is not a valid X.509 DisplayText until
// initialized with a value.
OPENSSL_EXPORT ASN1_STRING *DISPLAYTEXT_new(void);

// DISPLAYTEXT_free calls |ASN1_STRING_free|.
OPENSSL_EXPORT void DISPLAYTEXT_free(ASN1_STRING *str);

// d2i_DISPLAYTEXT parses up to |len| bytes from |*inp| as a DER-encoded X.509
// DisplayText (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
//
// TODO(https://crbug.com/boringssl/449): DisplayText's size limits are not
// currently enforced.
OPENSSL_EXPORT ASN1_STRING *d2i_DISPLAYTEXT(ASN1_STRING **out,
                                            const uint8_t **inp, long len);

// i2d_DISPLAYTEXT marshals |in| as a DER-encoded X.509 DisplayText (RFC 5280),
// as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_DISPLAYTEXT(const ASN1_STRING *in, uint8_t **outp);

// DISPLAYTEXT is an |ASN1_ITEM| whose ASN.1 type is X.509 DisplayText (RFC
// 5280) and C type is |ASN1_STRING*|.
DECLARE_ASN1_ITEM(DISPLAYTEXT)


// Bit strings.
//
// An ASN.1 BIT STRING type represents a string of bits. The string may not
// necessarily be a whole number of bytes. BIT STRINGs occur in ASN.1 structures
// in several forms:
//
// Some BIT STRINGs represent a bitmask of named bits, such as the X.509 key
// usage extension in RFC 5280, section 4.2.1.3. For such bit strings, DER
// imposes an additional restriction that trailing zero bits are removed. Some
// functions like |ASN1_BIT_STRING_set_bit| help in maintaining this.
//
// Other BIT STRINGs are arbitrary strings of bits used as identifiers and do
// not have this constraint, such as the X.509 issuerUniqueID field.
//
// Finally, some structures use BIT STRINGs as a container for byte strings. For
// example, the signatureValue field in X.509 and the subjectPublicKey field in
// SubjectPublicKeyInfo are defined as BIT STRINGs with a value specific to the
// AlgorithmIdentifier. While some unknown algorithm could choose to store
// arbitrary bit strings, all supported algorithms use a byte string, with bit
// order matching the DER encoding. Callers interpreting a BIT STRING as a byte
// string should use |ASN1_BIT_STRING_num_bytes| instead of |ASN1_STRING_length|
// and reject bit strings that are not a whole number of bytes.
//
// This library represents BIT STRINGs as |ASN1_STRING|s with type
// |V_ASN1_BIT_STRING|. The data contains the encoded form of the BIT STRING,
// including any padding bits added to round to a whole number of bytes, but
// excluding the leading byte containing the number of padding bits. If
// |ASN1_STRING_FLAG_BITS_LEFT| is set, the bottom three bits contains the
// number of padding bits. For example, DER encodes the BIT STRING {1, 0} as
// {0x06, 0x80 = 0b10_000000}. The |ASN1_STRING| representation has data of
// {0x80} and flags of ASN1_STRING_FLAG_BITS_LEFT | 6. If
// |ASN1_STRING_FLAG_BITS_LEFT| is unset, trailing zero bits are implicitly
// removed. Callers should not rely this representation when constructing bit
// strings. The padding bits in the |ASN1_STRING| data must be zero.

// ASN1_BIT_STRING_new calls |ASN1_STRING_type_new| with |V_ASN1_BIT_STRING|.
OPENSSL_EXPORT ASN1_BIT_STRING *ASN1_BIT_STRING_new(void);

// ASN1_BIT_STRING_free calls |ASN1_STRING_free|.
OPENSSL_EXPORT void ASN1_BIT_STRING_free(ASN1_BIT_STRING *str);

// d2i_ASN1_BIT_STRING parses up to |len| bytes from |*inp| as a DER-encoded
// ASN.1 BIT STRING, as described in |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_BIT_STRING *d2i_ASN1_BIT_STRING(ASN1_BIT_STRING **out,
                                                    const uint8_t **inp,
                                                    long len);

// i2d_ASN1_BIT_STRING marshals |in| as a DER-encoded ASN.1 BIT STRING, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_BIT_STRING(const ASN1_BIT_STRING *in,
                                       uint8_t **outp);

// c2i_ASN1_BIT_STRING decodes |len| bytes from |*inp| as the contents of a
// DER-encoded BIT STRING, excluding the tag and length. It behaves like
// |d2i_SAMPLE_with_reuse| except, on success, it always consumes all |len|
// bytes.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_BIT_STRING *c2i_ASN1_BIT_STRING(ASN1_BIT_STRING **out,
                                                    const uint8_t **inp,
                                                    long len);

// i2c_ASN1_BIT_STRING encodes |in| as the contents of a DER-encoded BIT STRING,
// excluding the tag and length. If |outp| is non-NULL, it writes the result to
// |*outp|, advances |*outp| just past the output, and returns the number of
// bytes written. |*outp| must have space available for the result. If |outp| is
// NULL, it returns the number of bytes without writing anything. On error, it
// returns a value <= 0.
//
// Note this function differs slightly from |i2d_SAMPLE|. If |outp| is non-NULL
// and |*outp| is NULL, it does not allocate a new buffer.
//
// TODO(davidben): This function currently returns zero on error instead of -1,
// but it is also mostly infallible. I've currently documented <= 0 to suggest
// callers work with both.
OPENSSL_EXPORT int i2c_ASN1_BIT_STRING(const ASN1_BIT_STRING *in,
                                       uint8_t **outp);

// ASN1_BIT_STRING is an |ASN1_ITEM| with ASN.1 type BIT STRING and C type
// |ASN1_BIT_STRING*|.
DECLARE_ASN1_ITEM(ASN1_BIT_STRING)

// ASN1_BIT_STRING_num_bytes computes the length of |str| in bytes. If |str|'s
// bit length is a multiple of 8, it sets |*out| to the byte length and returns
// one. Otherwise, it returns zero.
//
// This function may be used with |ASN1_STRING_get0_data| to interpret |str| as
// a byte string.
OPENSSL_EXPORT int ASN1_BIT_STRING_num_bytes(const ASN1_BIT_STRING *str,
                                             size_t *out);

// ASN1_BIT_STRING_set calls |ASN1_STRING_set|. It leaves flags unchanged, so
// the caller must set the number of unused bits.
//
// TODO(davidben): Maybe it should? Wrapping a byte string in a bit string is a
// common use case.
OPENSSL_EXPORT int ASN1_BIT_STRING_set(ASN1_BIT_STRING *str,
                                       const unsigned char *d,
                                       ossl_ssize_t length);

// ASN1_BIT_STRING_set_bit sets bit |n| of |str| to one if |value| is non-zero
// and zero if |value| is zero, resizing |str| as needed. It then truncates
// trailing zeros in |str| to align with the DER represention for a bit string
// with named bits. It returns one on success and zero on error. |n| is indexed
// beginning from zero.
OPENSSL_EXPORT int ASN1_BIT_STRING_set_bit(ASN1_BIT_STRING *str, int n,
                                           int value);

// ASN1_BIT_STRING_get_bit returns one if bit |n| of |a| is in bounds and set,
// and zero otherwise. |n| is indexed beginning from zero.
OPENSSL_EXPORT int ASN1_BIT_STRING_get_bit(const ASN1_BIT_STRING *str, int n);

// ASN1_BIT_STRING_check returns one if |str| only contains bits that are set in
// the |flags_len| bytes pointed by |flags|. Otherwise it returns zero. Bits in
// |flags| are arranged according to the DER representation, so bit 0
// corresponds to the MSB of |flags[0]|.
OPENSSL_EXPORT int ASN1_BIT_STRING_check(const ASN1_BIT_STRING *str,
                                         const unsigned char *flags,
                                         int flags_len);


// Integers and enumerated values.
//
// INTEGER and ENUMERATED values are represented as |ASN1_STRING|s where the
// data contains the big-endian encoding of the absolute value of the integer.
// The sign bit is encoded in the type: non-negative values have a type of
// |V_ASN1_INTEGER| or |V_ASN1_ENUMERATED|, while negative values have a type of
// |V_ASN1_NEG_INTEGER| or |V_ASN1_NEG_ENUMERATED|. Note this differs from DER's
// two's complement representation.
//
// The data in the |ASN1_STRING| may not have leading zeros. Note this means
// zero is represented as the empty string. Parsing functions will never return
// invalid representations. If an invalid input is constructed, the marshaling
// functions will skip leading zeros, however other functions, such as
// |ASN1_INTEGER_cmp| or |ASN1_INTEGER_get|, may not return the correct result.

DEFINE_STACK_OF(ASN1_INTEGER)

// ASN1_INTEGER_new calls |ASN1_STRING_type_new| with |V_ASN1_INTEGER|. The
// resulting object has value zero.
OPENSSL_EXPORT ASN1_INTEGER *ASN1_INTEGER_new(void);

// ASN1_INTEGER_free calls |ASN1_STRING_free|.
OPENSSL_EXPORT void ASN1_INTEGER_free(ASN1_INTEGER *str);

// ASN1_INTEGER_dup calls |ASN1_STRING_dup|.
OPENSSL_EXPORT ASN1_INTEGER *ASN1_INTEGER_dup(const ASN1_INTEGER *x);

// d2i_ASN1_INTEGER parses up to |len| bytes from |*inp| as a DER-encoded
// ASN.1 INTEGER, as described in |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_INTEGER *d2i_ASN1_INTEGER(ASN1_INTEGER **out,
                                              const uint8_t **inp, long len);

// i2d_ASN1_INTEGER marshals |in| as a DER-encoded ASN.1 INTEGER, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_INTEGER(const ASN1_INTEGER *in, uint8_t **outp);

// c2i_ASN1_INTEGER decodes |len| bytes from |*inp| as the contents of a
// DER-encoded INTEGER, excluding the tag and length. It behaves like
// |d2i_SAMPLE_with_reuse| except, on success, it always consumes all |len|
// bytes.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// some invalid inputs, but this will be removed in the future.
OPENSSL_EXPORT ASN1_INTEGER *c2i_ASN1_INTEGER(ASN1_INTEGER **in,
                                              const uint8_t **outp, long len);

// i2c_ASN1_INTEGER encodes |in| as the contents of a DER-encoded INTEGER,
// excluding the tag and length. If |outp| is non-NULL, it writes the result to
// |*outp|, advances |*outp| just past the output, and returns the number of
// bytes written. |*outp| must have space available for the result. If |outp| is
// NULL, it returns the number of bytes without writing anything. On error, it
// returns a value <= 0.
//
// Note this function differs slightly from |i2d_SAMPLE|. If |outp| is non-NULL
// and |*outp| is NULL, it does not allocate a new buffer.
//
// TODO(davidben): This function currently returns zero on error instead of -1,
// but it is also mostly infallible. I've currently documented <= 0 to suggest
// callers work with both.
OPENSSL_EXPORT int i2c_ASN1_INTEGER(const ASN1_INTEGER *in, uint8_t **outp);

// ASN1_INTEGER is an |ASN1_ITEM| with ASN.1 type INTEGER and C type
// |ASN1_INTEGER*|.
DECLARE_ASN1_ITEM(ASN1_INTEGER)

// ASN1_INTEGER_set_uint64 sets |a| to an INTEGER with value |v|. It returns one
// on success and zero on error.
OPENSSL_EXPORT int ASN1_INTEGER_set_uint64(ASN1_INTEGER *out, uint64_t v);

// ASN1_INTEGER_set_int64 sets |a| to an INTEGER with value |v|. It returns one
// on success and zero on error.
OPENSSL_EXPORT int ASN1_INTEGER_set_int64(ASN1_INTEGER *out, int64_t v);

// ASN1_INTEGER_get_uint64 converts |a| to a |uint64_t|. On success, it returns
// one and sets |*out| to the result. If |a| did not fit or has the wrong type,
// it returns zero.
OPENSSL_EXPORT int ASN1_INTEGER_get_uint64(uint64_t *out,
                                           const ASN1_INTEGER *a);

// ASN1_INTEGER_get_int64 converts |a| to a |int64_t|. On success, it returns
// one and sets |*out| to the result. If |a| did not fit or has the wrong type,
// it returns zero.
OPENSSL_EXPORT int ASN1_INTEGER_get_int64(int64_t *out, const ASN1_INTEGER *a);

// BN_to_ASN1_INTEGER sets |ai| to an INTEGER with value |bn| and returns |ai|
// on success or NULL or error. If |ai| is NULL, it returns a newly-allocated
// |ASN1_INTEGER| on success instead, which the caller must release with
// |ASN1_INTEGER_free|.
OPENSSL_EXPORT ASN1_INTEGER *BN_to_ASN1_INTEGER(const BIGNUM *bn,
                                                ASN1_INTEGER *ai);

// ASN1_INTEGER_to_BN sets |bn| to the value of |ai| and returns |bn| on success
// or NULL or error. If |bn| is NULL, it returns a newly-allocated |BIGNUM| on
// success instead, which the caller must release with |BN_free|.
OPENSSL_EXPORT BIGNUM *ASN1_INTEGER_to_BN(const ASN1_INTEGER *ai, BIGNUM *bn);

// ASN1_INTEGER_cmp compares the values of |x| and |y|. It returns an integer
// equal to, less than, or greater than zero if |x| is equal to, less than, or
// greater than |y|, respectively.
OPENSSL_EXPORT int ASN1_INTEGER_cmp(const ASN1_INTEGER *x,
                                    const ASN1_INTEGER *y);

// ASN1_ENUMERATED_new calls |ASN1_STRING_type_new| with |V_ASN1_ENUMERATED|.
// The resulting object has value zero.
OPENSSL_EXPORT ASN1_ENUMERATED *ASN1_ENUMERATED_new(void);

// ASN1_ENUMERATED_free calls |ASN1_STRING_free|.
OPENSSL_EXPORT void ASN1_ENUMERATED_free(ASN1_ENUMERATED *str);

// d2i_ASN1_ENUMERATED parses up to |len| bytes from |*inp| as a DER-encoded
// ASN.1 ENUMERATED, as described in |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_ENUMERATED *d2i_ASN1_ENUMERATED(ASN1_ENUMERATED **out,
                                                    const uint8_t **inp,
                                                    long len);

// i2d_ASN1_ENUMERATED marshals |in| as a DER-encoded ASN.1 ENUMERATED, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_ENUMERATED(const ASN1_ENUMERATED *in,
                                       uint8_t **outp);

// ASN1_ENUMERATED is an |ASN1_ITEM| with ASN.1 type ENUMERATED and C type
// |ASN1_ENUMERATED*|.
DECLARE_ASN1_ITEM(ASN1_ENUMERATED)

// ASN1_ENUMERATED_set_uint64 sets |a| to an ENUMERATED with value |v|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int ASN1_ENUMERATED_set_uint64(ASN1_ENUMERATED *out, uint64_t v);

// ASN1_ENUMERATED_set_int64 sets |a| to an ENUMERATED with value |v|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int ASN1_ENUMERATED_set_int64(ASN1_ENUMERATED *out, int64_t v);

// ASN1_ENUMERATED_get_uint64 converts |a| to a |uint64_t|. On success, it
// returns one and sets |*out| to the result. If |a| did not fit or has the
// wrong type, it returns zero.
OPENSSL_EXPORT int ASN1_ENUMERATED_get_uint64(uint64_t *out,
                                              const ASN1_ENUMERATED *a);

// ASN1_ENUMERATED_get_int64 converts |a| to a |int64_t|. On success, it
// returns one and sets |*out| to the result. If |a| did not fit or has the
// wrong type, it returns zero.
OPENSSL_EXPORT int ASN1_ENUMERATED_get_int64(int64_t *out,
                                             const ASN1_ENUMERATED *a);

// BN_to_ASN1_ENUMERATED sets |ai| to an ENUMERATED with value |bn| and returns
// |ai| on success or NULL or error. If |ai| is NULL, it returns a
// newly-allocated |ASN1_ENUMERATED| on success instead, which the caller must
// release with |ASN1_ENUMERATED_free|.
OPENSSL_EXPORT ASN1_ENUMERATED *BN_to_ASN1_ENUMERATED(const BIGNUM *bn,
                                                      ASN1_ENUMERATED *ai);

// ASN1_ENUMERATED_to_BN sets |bn| to the value of |ai| and returns |bn| on
// success or NULL or error. If |bn| is NULL, it returns a newly-allocated
// |BIGNUM| on success instead, which the caller must release with |BN_free|.
OPENSSL_EXPORT BIGNUM *ASN1_ENUMERATED_to_BN(const ASN1_ENUMERATED *ai,
                                             BIGNUM *bn);


// Time.
//
// GeneralizedTime and UTCTime values are represented as |ASN1_STRING|s. The
// type field is |V_ASN1_GENERALIZEDTIME| or |V_ASN1_UTCTIME|, respectively. The
// data field contains the DER encoding of the value. For example, the UNIX
// epoch would be "19700101000000Z" for a GeneralizedTime and "700101000000Z"
// for a UTCTime.
//
// ASN.1 does not define how to interpret UTCTime's two-digit year. RFC 5280
// defines it as a range from 1950 to 2049 for X.509. The library uses the
// RFC 5280 interpretation. It does not currently enforce the restrictions from
// BER, and the additional restrictions from RFC 5280, but future versions may.
// Callers should not rely on fractional seconds and non-UTC time zones.
//
// The |ASN1_TIME| typedef is a multi-string representing the X.509 Time type,
// which is a CHOICE of GeneralizedTime and UTCTime, using UTCTime when the
// value is in range.

// ASN1_UTCTIME_new calls |ASN1_STRING_type_new| with |V_ASN1_UTCTIME|. The
// resulting object contains empty contents and must be initialized to be a
// valid UTCTime.
OPENSSL_EXPORT ASN1_UTCTIME *ASN1_UTCTIME_new(void);

// ASN1_UTCTIME_free calls |ASN1_STRING_free|.
OPENSSL_EXPORT void ASN1_UTCTIME_free(ASN1_UTCTIME *str);

// d2i_ASN1_UTCTIME parses up to |len| bytes from |*inp| as a DER-encoded
// ASN.1 UTCTime, as described in |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_UTCTIME *d2i_ASN1_UTCTIME(ASN1_UTCTIME **out,
                                              const uint8_t **inp, long len);

// i2d_ASN1_UTCTIME marshals |in| as a DER-encoded ASN.1 UTCTime, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_UTCTIME(const ASN1_UTCTIME *in, uint8_t **outp);

// ASN1_UTCTIME is an |ASN1_ITEM| with ASN.1 type UTCTime and C type
// |ASN1_UTCTIME*|.
DECLARE_ASN1_ITEM(ASN1_UTCTIME)

// ASN1_UTCTIME_check returns one if |a| is a valid UTCTime and zero otherwise.
OPENSSL_EXPORT int ASN1_UTCTIME_check(const ASN1_UTCTIME *a);

// ASN1_UTCTIME_set represents |posix_time| as a UTCTime and writes the result
// to |s|. It returns |s| on success and NULL on error. If |s| is NULL, it
// returns a newly-allocated |ASN1_UTCTIME| instead.
//
// Note this function may fail if the time is out of range for UTCTime.
OPENSSL_EXPORT ASN1_UTCTIME *ASN1_UTCTIME_set(ASN1_UTCTIME *s,
                                              int64_t posix_time);

// ASN1_UTCTIME_adj adds |offset_day| days and |offset_sec| seconds to
// |posix_time| and writes the result to |s| as a UTCTime. It returns |s| on
// success and NULL on error. If |s| is NULL, it returns a newly-allocated
// |ASN1_UTCTIME| instead.
//
// Note this function may fail if the time overflows or is out of range for
// UTCTime.
OPENSSL_EXPORT ASN1_UTCTIME *ASN1_UTCTIME_adj(ASN1_UTCTIME *s,
                                              int64_t posix_time,
                                              int offset_day, long offset_sec);

// ASN1_UTCTIME_set_string sets |s| to a UTCTime whose contents are a copy of
// |str|. It returns one on success and zero on error or if |str| is not a valid
// UTCTime.
//
// If |s| is NULL, this function validates |str| without copying it.
OPENSSL_EXPORT int ASN1_UTCTIME_set_string(ASN1_UTCTIME *s, const char *str);

// ASN1_UTCTIME_cmp_time_t compares |s| to |t|. It returns -1 if |s| < |t|, 0 if
// they are equal, 1 if |s| > |t|, and -2 on error.
OPENSSL_EXPORT int ASN1_UTCTIME_cmp_time_t(const ASN1_UTCTIME *s, time_t t);

// ASN1_GENERALIZEDTIME_new calls |ASN1_STRING_type_new| with
// |V_ASN1_GENERALIZEDTIME|. The resulting object contains empty contents and
// must be initialized to be a valid GeneralizedTime.
OPENSSL_EXPORT ASN1_GENERALIZEDTIME *ASN1_GENERALIZEDTIME_new(void);

// ASN1_GENERALIZEDTIME_free calls |ASN1_STRING_free|.
OPENSSL_EXPORT void ASN1_GENERALIZEDTIME_free(ASN1_GENERALIZEDTIME *str);

// d2i_ASN1_GENERALIZEDTIME parses up to |len| bytes from |*inp| as a
// DER-encoded ASN.1 GeneralizedTime, as described in |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_GENERALIZEDTIME *d2i_ASN1_GENERALIZEDTIME(
    ASN1_GENERALIZEDTIME **out, const uint8_t **inp, long len);

// i2d_ASN1_GENERALIZEDTIME marshals |in| as a DER-encoded ASN.1
// GeneralizedTime, as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_GENERALIZEDTIME(const ASN1_GENERALIZEDTIME *in,
                                            uint8_t **outp);

// ASN1_GENERALIZEDTIME is an |ASN1_ITEM| with ASN.1 type GeneralizedTime and C
// type |ASN1_GENERALIZEDTIME*|.
DECLARE_ASN1_ITEM(ASN1_GENERALIZEDTIME)

// ASN1_GENERALIZEDTIME_check returns one if |a| is a valid GeneralizedTime and
// zero otherwise.
OPENSSL_EXPORT int ASN1_GENERALIZEDTIME_check(const ASN1_GENERALIZEDTIME *a);

// ASN1_GENERALIZEDTIME_set represents |posix_time| as a GeneralizedTime and
// writes the result to |s|. It returns |s| on success and NULL on error. If |s|
// is NULL, it returns a newly-allocated |ASN1_GENERALIZEDTIME| instead.
//
// Note this function may fail if the time is out of range for GeneralizedTime.
OPENSSL_EXPORT ASN1_GENERALIZEDTIME *ASN1_GENERALIZEDTIME_set(
    ASN1_GENERALIZEDTIME *s, int64_t posix_time);

// ASN1_GENERALIZEDTIME_adj adds |offset_day| days and |offset_sec| seconds to
// |posix_time| and writes the result to |s| as a GeneralizedTime. It returns
// |s| on success and NULL on error. If |s| is NULL, it returns a
// newly-allocated |ASN1_GENERALIZEDTIME| instead.
//
// Note this function may fail if the time overflows or is out of range for
// GeneralizedTime.
OPENSSL_EXPORT ASN1_GENERALIZEDTIME *ASN1_GENERALIZEDTIME_adj(
    ASN1_GENERALIZEDTIME *s, int64_t posix_time, int offset_day,
    long offset_sec);

// ASN1_GENERALIZEDTIME_set_string sets |s| to a GeneralizedTime whose contents
// are a copy of |str|. It returns one on success and zero on error or if |str|
// is not a valid GeneralizedTime.
//
// If |s| is NULL, this function validates |str| without copying it.
OPENSSL_EXPORT int ASN1_GENERALIZEDTIME_set_string(ASN1_GENERALIZEDTIME *s,
                                                   const char *str);

// B_ASN1_TIME is a bitmask of types allowed in an X.509 Time.
#define B_ASN1_TIME (B_ASN1_UTCTIME | B_ASN1_GENERALIZEDTIME)

// ASN1_TIME_new returns a newly-allocated |ASN1_TIME| with type -1, or NULL on
// error. The resulting |ASN1_TIME| is not a valid X.509 Time until initialized
// with a value.
OPENSSL_EXPORT ASN1_TIME *ASN1_TIME_new(void);

// ASN1_TIME_free releases memory associated with |str|.
OPENSSL_EXPORT void ASN1_TIME_free(ASN1_TIME *str);

// d2i_ASN1_TIME parses up to |len| bytes from |*inp| as a DER-encoded X.509
// Time (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_TIME *d2i_ASN1_TIME(ASN1_TIME **out, const uint8_t **inp,
                                        long len);

// i2d_ASN1_TIME marshals |in| as a DER-encoded X.509 Time (RFC 5280), as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_TIME(const ASN1_TIME *in, uint8_t **outp);

// ASN1_TIME is an |ASN1_ITEM| whose ASN.1 type is X.509 Time (RFC 5280) and C
// type is |ASN1_TIME*|.
DECLARE_ASN1_ITEM(ASN1_TIME)

// ASN1_TIME_diff computes |to| - |from|. On success, it sets |*out_days| to the
// difference in days, rounded towards zero, sets |*out_seconds| to the
// remainder, and returns one. On error, it returns zero.
//
// If |from| is before |to|, both outputs will be <= 0, with at least one
// negative. If |from| is after |to|, both will be >= 0, with at least one
// positive. If they are equal, ignoring fractional seconds, both will be zero.
//
// Note this function may fail on overflow, or if |from| or |to| cannot be
// decoded.
OPENSSL_EXPORT int ASN1_TIME_diff(int *out_days, int *out_seconds,
                                  const ASN1_TIME *from, const ASN1_TIME *to);

// ASN1_TIME_set_posix represents |posix_time| as a GeneralizedTime or UTCTime
// and writes the result to |s|. As in RFC 5280, section 4.1.2.5, it uses
// UTCTime when the time fits and GeneralizedTime otherwise. It returns |s| on
// success and NULL on error. If |s| is NULL, it returns a newly-allocated
// |ASN1_TIME| instead.
//
// Note this function may fail if the time is out of range for GeneralizedTime.
OPENSSL_EXPORT ASN1_TIME *ASN1_TIME_set_posix(ASN1_TIME *s, int64_t posix_time);

// ASN1_TIME_set is exactly the same as |ASN1_TIME_set_posix| but with a
// time_t as input for compatibility.
OPENSSL_EXPORT ASN1_TIME *ASN1_TIME_set(ASN1_TIME *s, time_t time);

// ASN1_TIME_adj adds |offset_day| days and |offset_sec| seconds to
// |posix_time| and writes the result to |s|. As in RFC 5280, section 4.1.2.5,
// it uses UTCTime when the time fits and GeneralizedTime otherwise. It returns
// |s| on success and NULL on error. If |s| is NULL, it returns a
// newly-allocated |ASN1_GENERALIZEDTIME| instead.
//
// Note this function may fail if the time overflows or is out of range for
// GeneralizedTime.
OPENSSL_EXPORT ASN1_TIME *ASN1_TIME_adj(ASN1_TIME *s, int64_t posix_time,
                                        int offset_day, long offset_sec);

// ASN1_TIME_check returns one if |t| is a valid UTCTime or GeneralizedTime, and
// zero otherwise. |t|'s type determines which check is performed. This
// function does not enforce that UTCTime was used when possible.
OPENSSL_EXPORT int ASN1_TIME_check(const ASN1_TIME *t);

// ASN1_TIME_to_generalizedtime converts |t| to a GeneralizedTime. If |out| is
// NULL, it returns a newly-allocated |ASN1_GENERALIZEDTIME| on success, or NULL
// on error. If |out| is non-NULL and |*out| is NULL, it additionally sets
// |*out| to the result. If |out| and |*out| are non-NULL, it instead updates
// the object pointed by |*out| and returns |*out| on success or NULL on error.
OPENSSL_EXPORT ASN1_GENERALIZEDTIME *ASN1_TIME_to_generalizedtime(
    const ASN1_TIME *t, ASN1_GENERALIZEDTIME **out);

// ASN1_TIME_set_string behaves like |ASN1_UTCTIME_set_string| if |str| is a
// valid UTCTime, and |ASN1_GENERALIZEDTIME_set_string| if |str| is a valid
// GeneralizedTime. If |str| is neither, it returns zero.
OPENSSL_EXPORT int ASN1_TIME_set_string(ASN1_TIME *s, const char *str);


// ASN1_TIME conversion functions.
//
// |struct| |tm| represents a calendar date: year, month, day... it is not
// necessarily a valid day, e.g. month 13. |time_t| is a typedef for the
// system's type that represents the seconds since the UNIX epoch. Posix time is
// a signed 64-bit integer which also represents the seconds since the UNIX
// epoch.

// ASN1_TIME_to_tm converts the ASN1 time |t| to the calendar day representation
// and writes it to |out|. If |t| is NULL, then the current time is converted.
// The output time is GMT. The tm_sec, tm_min, tm_hour, tm_mday, tm_mon and
// tm_year fields of |out| are set to proper values, all other fields are set
// to 0. If tm is NULL this function performs a format check on |t| only.
OPENSSL_EXPORT int ASN1_TIME_to_tm(const ASN1_TIME *t, struct tm *out);

// ASN1_TIME_set_string_X509 behaves like |ASN1_TIME_set_string| except it
// additionally converts GeneralizedTime to UTCTime if it is in the range where
// UTCTime is used. See RFC 5280, section 4.1.2.5.
OPENSSL_EXPORT int ASN1_TIME_set_string_X509(ASN1_TIME *s, const char *str);

// ASN1_TIME_to_time_t converts |t| to a time_t value in |out|. On
// success, one is returned. On failure zero is returned. This function
// will fail if the time can not be represented in a time_t.
OPENSSL_EXPORT int ASN1_TIME_to_time_t(const ASN1_TIME *t, time_t *out);

// ASN1_TIME_to_posix converts |t| to a POSIX time value in |out|. On
// success, one is returned. On failure zero is returned.
OPENSSL_EXPORT int ASN1_TIME_to_posix(const ASN1_TIME *t, int64_t *out);

// TODO(davidben): Expand and document function prototypes generated in macros.


// NULL values.
//
// This library represents the ASN.1 NULL value by a non-NULL pointer to the
// opaque type |ASN1_NULL|. An omitted OPTIONAL ASN.1 NULL value is a NULL
// pointer. Unlike other pointer types, it is not necessary to free |ASN1_NULL|
// pointers, but it is safe to do so.

// ASN1_NULL_new returns an opaque, non-NULL pointer. It is safe to call
// |ASN1_NULL_free| on the result, but not necessary.
OPENSSL_EXPORT ASN1_NULL *ASN1_NULL_new(void);

// ASN1_NULL_free does nothing.
OPENSSL_EXPORT void ASN1_NULL_free(ASN1_NULL *null);

// d2i_ASN1_NULL parses a DER-encoded ASN.1 NULL value from up to |len| bytes
// at |*inp|, as described in |d2i_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_NULL *d2i_ASN1_NULL(ASN1_NULL **out, const uint8_t **inp,
                                        long len);

// i2d_ASN1_NULL marshals |in| as a DER-encoded ASN.1 NULL value, as described
// in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_NULL(const ASN1_NULL *in, uint8_t **outp);

// ASN1_NULL is an |ASN1_ITEM| with ASN.1 type NULL and C type |ASN1_NULL*|.
DECLARE_ASN1_ITEM(ASN1_NULL)


// Object identifiers.
//
// An |ASN1_OBJECT| represents a ASN.1 OBJECT IDENTIFIER. See also obj.h for
// additional functions relating to |ASN1_OBJECT|.
//
// TODO(davidben): What's the relationship between asn1.h and obj.h? Most of
// obj.h deals with the large NID table, but then functions like |OBJ_get0_data|
// or |OBJ_dup| are general |ASN1_OBJECT| functions.

DEFINE_STACK_OF(ASN1_OBJECT)

// ASN1_OBJECT_create returns a newly-allocated |ASN1_OBJECT| with |len| bytes
// from |data| as the encoded OID, or NULL on error. |data| should contain the
// DER-encoded identifier, excluding the tag and length.
//
// |nid| should be |NID_undef|. Passing a NID value that does not match |data|
// will cause some functions to misbehave. |sn| and |ln| should be NULL. If
// non-NULL, they are stored as short and long names, respectively, but these
// values have no effect for |ASN1_OBJECT|s created through this function.
//
// TODO(davidben): Should we just ignore all those parameters? NIDs and names
// are only relevant for |ASN1_OBJECT|s in the obj.h table.
OPENSSL_EXPORT ASN1_OBJECT *ASN1_OBJECT_create(int nid, const uint8_t *data,
                                               size_t len, const char *sn,
                                               const char *ln);

// ASN1_OBJECT_free releases memory associated with |a|. If |a| is a static
// |ASN1_OBJECT|, returned from |OBJ_nid2obj|, this function does nothing.
OPENSSL_EXPORT void ASN1_OBJECT_free(ASN1_OBJECT *a);

// d2i_ASN1_OBJECT parses a DER-encoded ASN.1 OBJECT IDENTIFIER from up to |len|
// bytes at |*inp|, as described in |d2i_SAMPLE|.
OPENSSL_EXPORT ASN1_OBJECT *d2i_ASN1_OBJECT(ASN1_OBJECT **out,
                                            const uint8_t **inp, long len);

// i2d_ASN1_OBJECT marshals |in| as a DER-encoded ASN.1 OBJECT IDENTIFIER, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_OBJECT(const ASN1_OBJECT *in, uint8_t **outp);

// c2i_ASN1_OBJECT decodes |len| bytes from |*inp| as the contents of a
// DER-encoded OBJECT IDENTIFIER, excluding the tag and length. It behaves like
// |d2i_SAMPLE_with_reuse| except, on success, it always consumes all |len|
// bytes.
OPENSSL_EXPORT ASN1_OBJECT *c2i_ASN1_OBJECT(ASN1_OBJECT **out,
                                            const uint8_t **inp, long len);

// ASN1_OBJECT is an |ASN1_ITEM| with ASN.1 type OBJECT IDENTIFIER and C type
// |ASN1_OBJECT*|.
DECLARE_ASN1_ITEM(ASN1_OBJECT)


// Arbitrary elements.

// An asn1_type_st (aka |ASN1_TYPE|) represents an arbitrary ASN.1 element,
// typically used for ANY types. It contains a |type| field and a |value| union
// dependent on |type|.
//
// WARNING: This struct has a complex representation. Callers must not construct
// |ASN1_TYPE| values manually. Use |ASN1_TYPE_set| and |ASN1_TYPE_set1|
// instead. Additionally, callers performing non-trivial operations on this type
// are encouraged to use |CBS| and |CBB| from <openssl/bytestring.h>, and
// convert to or from |ASN1_TYPE| with |d2i_ASN1_TYPE| or |i2d_ASN1_TYPE|.
//
// The |type| field corresponds to the tag of the ASN.1 element being
// represented:
//
// If |type| is a |V_ASN1_*| constant for an ASN.1 string-like type, as defined
// by |ASN1_STRING|, the tag matches the constant. |value| contains an
// |ASN1_STRING| pointer (equivalently, one of the more specific typedefs). See
// |ASN1_STRING| for details on the representation. Unlike |ASN1_STRING|,
// |ASN1_TYPE| does not use the |V_ASN1_NEG| flag for negative INTEGER and
// ENUMERATE values. For a negative value, the |ASN1_TYPE|'s |type| will be
// |V_ASN1_INTEGER| or |V_ASN1_ENUMERATED|, but |value| will an |ASN1_STRING|
// whose |type| is |V_ASN1_NEG_INTEGER| or |V_ASN1_NEG_ENUMERATED|.
//
// If |type| is |V_ASN1_OBJECT|, the tag is OBJECT IDENTIFIER and |value|
// contains an |ASN1_OBJECT| pointer.
//
// If |type| is |V_ASN1_NULL|, the tag is NULL. |value| contains a NULL pointer.
//
// If |type| is |V_ASN1_BOOLEAN|, the tag is BOOLEAN. |value| contains an
// |ASN1_BOOLEAN|.
//
// If |type| is |V_ASN1_SEQUENCE|, |V_ASN1_SET|, or |V_ASN1_OTHER|, the tag is
// SEQUENCE, SET, or some non-universal tag, respectively. |value| is an
// |ASN1_STRING| containing the entire element, including the tag and length.
// The |ASN1_STRING|'s |type| field matches the containing |ASN1_TYPE|'s |type|.
//
// Other positive values of |type|, up to |V_ASN1_MAX_UNIVERSAL|, correspond to
// universal primitive tags not directly supported by this library. |value| is
// an |ASN1_STRING| containing the body of the element, excluding the tag
// and length. The |ASN1_STRING|'s |type| field matches the containing
// |ASN1_TYPE|'s |type|.
struct asn1_type_st {
  int type;
  union {
    char *ptr;
    ASN1_BOOLEAN boolean;
    ASN1_STRING *asn1_string;
    ASN1_OBJECT *object;
    ASN1_INTEGER *integer;
    ASN1_ENUMERATED *enumerated;
    ASN1_BIT_STRING *bit_string;
    ASN1_OCTET_STRING *octet_string;
    ASN1_PRINTABLESTRING *printablestring;
    ASN1_T61STRING *t61string;
    ASN1_IA5STRING *ia5string;
    ASN1_GENERALSTRING *generalstring;
    ASN1_BMPSTRING *bmpstring;
    ASN1_UNIVERSALSTRING *universalstring;
    ASN1_UTCTIME *utctime;
    ASN1_GENERALIZEDTIME *generalizedtime;
    ASN1_VISIBLESTRING *visiblestring;
    ASN1_UTF8STRING *utf8string;
    // set and sequence are left complete and still contain the entire element.
    ASN1_STRING *set;
    ASN1_STRING *sequence;
    ASN1_VALUE *asn1_value;
  } value;
};

DEFINE_STACK_OF(ASN1_TYPE)

// ASN1_TYPE_new returns a newly-allocated |ASN1_TYPE|, or NULL on allocation
// failure. The resulting object has type -1 and must be initialized to be
// a valid ANY value.
OPENSSL_EXPORT ASN1_TYPE *ASN1_TYPE_new(void);

// ASN1_TYPE_free releases memory associated with |a|.
OPENSSL_EXPORT void ASN1_TYPE_free(ASN1_TYPE *a);

// d2i_ASN1_TYPE parses up to |len| bytes from |*inp| as an ASN.1 value of any
// type, as described in |d2i_SAMPLE_with_reuse|. Note this function only
// validates primitive, universal types supported by this library. Values of
// type |V_ASN1_SEQUENCE|, |V_ASN1_SET|, |V_ASN1_OTHER|, or an unsupported
// primitive type must be validated by the caller when interpreting.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_TYPE *d2i_ASN1_TYPE(ASN1_TYPE **out, const uint8_t **inp,
                                        long len);

// i2d_ASN1_TYPE marshals |in| as DER, as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_TYPE(const ASN1_TYPE *in, uint8_t **outp);

// ASN1_ANY is an |ASN1_ITEM| with ASN.1 type ANY and C type |ASN1_TYPE*|. Note
// the |ASN1_ITEM| name and C type do not match.
DECLARE_ASN1_ITEM(ASN1_ANY)

// ASN1_TYPE_get returns the type of |a|, which will be one of the |V_ASN1_*|
// constants, or zero if |a| is not fully initialized.
OPENSSL_EXPORT int ASN1_TYPE_get(const ASN1_TYPE *a);

// ASN1_TYPE_set sets |a| to an |ASN1_TYPE| of type |type| and value |value|,
// releasing the previous contents of |a|.
//
// If |type| is |V_ASN1_BOOLEAN|, |a| is set to FALSE if |value| is NULL and
// TRUE otherwise. If setting |a| to TRUE, |value| may be an invalid pointer,
// such as (void*)1.
//
// If |type| is |V_ASN1_NULL|, |value| must be NULL.
//
// For other values of |type|, this function takes ownership of |value|, which
// must point to an object of the corresponding type. See |ASN1_TYPE| for
// details.
OPENSSL_EXPORT void ASN1_TYPE_set(ASN1_TYPE *a, int type, void *value);

// ASN1_TYPE_set1 behaves like |ASN1_TYPE_set| except it does not take ownership
// of |value|. It returns one on success and zero on error.
OPENSSL_EXPORT int ASN1_TYPE_set1(ASN1_TYPE *a, int type, const void *value);

// ASN1_TYPE_cmp returns zero if |a| and |b| are equal and some non-zero value
// otherwise. Note this function can only be used for equality checks, not an
// ordering.
OPENSSL_EXPORT int ASN1_TYPE_cmp(const ASN1_TYPE *a, const ASN1_TYPE *b);

typedef STACK_OF(ASN1_TYPE) ASN1_SEQUENCE_ANY;

// d2i_ASN1_SEQUENCE_ANY parses up to |len| bytes from |*inp| as a DER-encoded
// ASN.1 SEQUENCE OF ANY structure, as described in |d2i_SAMPLE_with_reuse|. The
// resulting |ASN1_SEQUENCE_ANY| owns its contents and thus must be released
// with |sk_ASN1_TYPE_pop_free| and |ASN1_TYPE_free|, not |sk_ASN1_TYPE_free|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_SEQUENCE_ANY *d2i_ASN1_SEQUENCE_ANY(ASN1_SEQUENCE_ANY **out,
                                                        const uint8_t **inp,
                                                        long len);

// i2d_ASN1_SEQUENCE_ANY marshals |in| as a DER-encoded SEQUENCE OF ANY
// structure, as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_SEQUENCE_ANY(const ASN1_SEQUENCE_ANY *in,
                                         uint8_t **outp);

// d2i_ASN1_SET_ANY parses up to |len| bytes from |*inp| as a DER-encoded ASN.1
// SET OF ANY structure, as described in |d2i_SAMPLE_with_reuse|. The resulting
// |ASN1_SEQUENCE_ANY| owns its contents and thus must be released with
// |sk_ASN1_TYPE_pop_free| and |ASN1_TYPE_free|, not |sk_ASN1_TYPE_free|.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_SEQUENCE_ANY *d2i_ASN1_SET_ANY(ASN1_SEQUENCE_ANY **out,
                                                   const uint8_t **inp,
                                                   long len);

// i2d_ASN1_SET_ANY marshals |in| as a DER-encoded SET OF ANY structure, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_ASN1_SET_ANY(const ASN1_SEQUENCE_ANY *in,
                                    uint8_t **outp);


// Human-readable output.
//
// The following functions output types in some human-readable format. These
// functions may be used for debugging and logging. However, the output should
// not be consumed programmatically. They may be ambiguous or lose information.

// ASN1_UTCTIME_print writes a human-readable representation of |a| to |out|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int ASN1_UTCTIME_print(BIO *out, const ASN1_UTCTIME *a);

// ASN1_GENERALIZEDTIME_print writes a human-readable representation of |a| to
// |out|. It returns one on success and zero on error.
OPENSSL_EXPORT int ASN1_GENERALIZEDTIME_print(BIO *out,
                                              const ASN1_GENERALIZEDTIME *a);

// ASN1_TIME_print writes a human-readable representation of |a| to |out|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int ASN1_TIME_print(BIO *out, const ASN1_TIME *a);

// ASN1_STRING_print writes a human-readable representation of |str| to |out|.
// It returns one on success and zero on error. Unprintable characters are
// replaced with '.'.
OPENSSL_EXPORT int ASN1_STRING_print(BIO *out, const ASN1_STRING *str);

// The following flags must not collide with |XN_FLAG_*|.

// ASN1_STRFLGS_ESC_2253 causes characters to be escaped as in RFC 2253, section
// 2.4.
#define ASN1_STRFLGS_ESC_2253 1ul

// ASN1_STRFLGS_ESC_CTRL causes all control characters to be escaped.
#define ASN1_STRFLGS_ESC_CTRL 2ul

// ASN1_STRFLGS_ESC_MSB causes all characters above 127 to be escaped.
#define ASN1_STRFLGS_ESC_MSB 4ul

// ASN1_STRFLGS_ESC_QUOTE causes the string to be surrounded by quotes, rather
// than using backslashes, when characters are escaped. Fewer characters will
// require escapes in this case.
#define ASN1_STRFLGS_ESC_QUOTE 8ul

// ASN1_STRFLGS_UTF8_CONVERT causes the string to be encoded as UTF-8, with each
// byte in the UTF-8 encoding treated as an individual character for purposes of
// escape sequences. If not set, each Unicode codepoint in the string is treated
// as a character, with wide characters escaped as "\Uxxxx" or "\Wxxxxxxxx".
// Note this can be ambiguous if |ASN1_STRFLGS_ESC_*| are all unset. In that
// case, backslashes are not escaped, but wide characters are.
#define ASN1_STRFLGS_UTF8_CONVERT 0x10ul

// ASN1_STRFLGS_IGNORE_TYPE causes the string type to be ignored. The
// |ASN1_STRING| in-memory representation will be printed directly.
#define ASN1_STRFLGS_IGNORE_TYPE 0x20ul

// ASN1_STRFLGS_SHOW_TYPE causes the string type to be included in the output.
#define ASN1_STRFLGS_SHOW_TYPE 0x40ul

// ASN1_STRFLGS_DUMP_ALL causes all strings to be printed as a hexdump, using
// RFC 2253 hexstring notation, such as "#0123456789ABCDEF".
#define ASN1_STRFLGS_DUMP_ALL 0x80ul

// ASN1_STRFLGS_DUMP_UNKNOWN behaves like |ASN1_STRFLGS_DUMP_ALL| but only
// applies to values of unknown type. If unset, unknown values will print
// their contents as single-byte characters with escape sequences.
#define ASN1_STRFLGS_DUMP_UNKNOWN 0x100ul

// ASN1_STRFLGS_DUMP_DER causes hexdumped strings (as determined by
// |ASN1_STRFLGS_DUMP_ALL| or |ASN1_STRFLGS_DUMP_UNKNOWN|) to print the entire
// DER element as in RFC 2253, rather than only the contents of the
// |ASN1_STRING|.
#define ASN1_STRFLGS_DUMP_DER 0x200ul

// ASN1_STRFLGS_RFC2253 causes the string to be escaped as in RFC 2253,
// additionally escaping control characters.
#define ASN1_STRFLGS_RFC2253                                              \
  (ASN1_STRFLGS_ESC_2253 | ASN1_STRFLGS_ESC_CTRL | ASN1_STRFLGS_ESC_MSB | \
   ASN1_STRFLGS_UTF8_CONVERT | ASN1_STRFLGS_DUMP_UNKNOWN |                \
   ASN1_STRFLGS_DUMP_DER)

// ASN1_STRING_print_ex writes a human-readable representation of |str| to
// |out|. It returns the number of bytes written on success and -1 on error. If
// |out| is NULL, it returns the number of bytes it would have written, without
// writing anything.
//
// The |flags| should be a combination of combination of |ASN1_STRFLGS_*|
// constants. See the documentation for each flag for how it controls the
// output. If unsure, use |ASN1_STRFLGS_RFC2253|.
OPENSSL_EXPORT int ASN1_STRING_print_ex(BIO *out, const ASN1_STRING *str,
                                        unsigned long flags);

// ASN1_STRING_print_ex_fp behaves like |ASN1_STRING_print_ex| but writes to a
// |FILE| rather than a |BIO|.
OPENSSL_EXPORT int ASN1_STRING_print_ex_fp(FILE *fp, const ASN1_STRING *str,
                                           unsigned long flags);

// i2a_ASN1_INTEGER writes a human-readable representation of |a| to |bp|. It
// returns the number of bytes written on success, or a negative number on
// error. On error, this function may have written a partial output to |bp|.
OPENSSL_EXPORT int i2a_ASN1_INTEGER(BIO *bp, const ASN1_INTEGER *a);

// i2a_ASN1_ENUMERATED writes a human-readable representation of |a| to |bp|. It
// returns the number of bytes written on success, or a negative number on
// error. On error, this function may have written a partial output to |bp|.
OPENSSL_EXPORT int i2a_ASN1_ENUMERATED(BIO *bp, const ASN1_ENUMERATED *a);

// i2a_ASN1_OBJECT writes a human-readable representation of |a| to |bp|. It
// returns the number of bytes written on success, or a negative number on
// error. On error, this function may have written a partial output to |bp|.
OPENSSL_EXPORT int i2a_ASN1_OBJECT(BIO *bp, const ASN1_OBJECT *a);

// i2a_ASN1_STRING writes a text representation of |a|'s contents to |bp|. It
// returns the number of bytes written on success, or a negative number on
// error. On error, this function may have written a partial output to |bp|.
// |type| is ignored.
//
// This function does not decode |a| into a Unicode string. It only hex-encodes
// the internal representation of |a|. This is suitable for printing an OCTET
// STRING, but may not be human-readable for any other string type.
OPENSSL_EXPORT int i2a_ASN1_STRING(BIO *bp, const ASN1_STRING *a, int type);

// i2t_ASN1_OBJECT calls |OBJ_obj2txt| with |always_return_oid| set to zero.
OPENSSL_EXPORT int i2t_ASN1_OBJECT(char *buf, int buf_len,
                                   const ASN1_OBJECT *a);


// Low-level encoding functions.

// ASN1_get_object parses a BER element from up to |max_len| bytes at |*inp|. It
// returns |V_ASN1_CONSTRUCTED| if it successfully parsed a constructed element,
// zero if it successfully parsed a primitive element, and 0x80 on error. On
// success, it additionally advances |*inp| to the element body, sets
// |*out_length|, |*out_tag|, and |*out_class| to the element's length, tag
// number, and tag class, respectively,
//
// Unlike OpenSSL, this function does not support indefinite-length elements.
//
// This function is difficult to use correctly. Use |CBS_get_asn1| and related
// functions from bytestring.h.
//
// TODO(https://crbug.com/boringssl/354): Remove support for non-minimal
// lengths.
OPENSSL_EXPORT int ASN1_get_object(const unsigned char **inp, long *out_length,
                                   int *out_tag, int *out_class, long max_len);

// ASN1_put_object writes the header for a DER or BER element to |*outp| and
// advances |*outp| by the number of bytes written. The caller is responsible
// for ensuring |*outp| has enough space for the output. The header describes an
// element with length |length|, tag number |tag|, and class |xclass|. |xclass|
// should be one of the |V_ASN1_*| tag class constants. The element is primitive
// if |constructed| is zero and constructed if it is one or two. If
// |constructed| is two, |length| is ignored and the element uses
// indefinite-length encoding.
//
// Use |CBB_add_asn1| instead.
OPENSSL_EXPORT void ASN1_put_object(unsigned char **outp, int constructed,
                                    int length, int tag, int xclass);

// ASN1_put_eoc writes two zero bytes to |*outp|, advances |*outp| to point past
// those bytes, and returns two.
//
// Use definite-length encoding instead.
OPENSSL_EXPORT int ASN1_put_eoc(unsigned char **outp);

// ASN1_object_size returns the number of bytes needed to encode a DER or BER
// value with length |length| and tag number |tag|, or -1 on error. |tag| should
// not include the constructed bit or tag class. If |constructed| is zero or
// one, the result uses a definite-length encoding with minimally-encoded
// length, as in DER. If |constructed| is two, the result uses BER
// indefinite-length encoding.
//
// Use |CBB_add_asn1| instead.
OPENSSL_EXPORT int ASN1_object_size(int constructed, int length, int tag);


// Function declaration macros.
//
// The following macros declare functions for ASN.1 types. Prefer writing the
// prototypes directly. Particularly when |type|, |itname|, or |name| differ,
// the macros can be difficult to understand.

#define DECLARE_ASN1_FUNCTIONS(type) DECLARE_ASN1_FUNCTIONS_name(type, type)

#define DECLARE_ASN1_ALLOC_FUNCTIONS(type) \
  DECLARE_ASN1_ALLOC_FUNCTIONS_name(type, type)

#define DECLARE_ASN1_FUNCTIONS_name(type, name) \
  DECLARE_ASN1_ALLOC_FUNCTIONS_name(type, name) \
  DECLARE_ASN1_ENCODE_FUNCTIONS(type, name, name)

#define DECLARE_ASN1_FUNCTIONS_fname(type, itname, name) \
  DECLARE_ASN1_ALLOC_FUNCTIONS_name(type, name)          \
  DECLARE_ASN1_ENCODE_FUNCTIONS(type, itname, name)

#define DECLARE_ASN1_ENCODE_FUNCTIONS(type, itname, name)             \
  OPENSSL_EXPORT type *d2i_##name(type **a, const unsigned char **in, \
                                  long len);                          \
  OPENSSL_EXPORT int i2d_##name(type *a, unsigned char **out);        \
  DECLARE_ASN1_ITEM(itname)

#define DECLARE_ASN1_ENCODE_FUNCTIONS_const(type, name)               \
  OPENSSL_EXPORT type *d2i_##name(type **a, const unsigned char **in, \
                                  long len);                          \
  OPENSSL_EXPORT int i2d_##name(const type *a, unsigned char **out);  \
  DECLARE_ASN1_ITEM(name)

#define DECLARE_ASN1_FUNCTIONS_const(name) \
  DECLARE_ASN1_ALLOC_FUNCTIONS(name)       \
  DECLARE_ASN1_ENCODE_FUNCTIONS_const(name, name)

#define DECLARE_ASN1_ALLOC_FUNCTIONS_name(type, name) \
  OPENSSL_EXPORT type *name##_new(void);              \
  OPENSSL_EXPORT void name##_free(type *a);


// Deprecated functions.

// M_ASN1_* are legacy aliases for various |ASN1_STRING| functions. Use the
// functions themselves.
#define M_ASN1_STRING_length(x) ASN1_STRING_length(x)
#define M_ASN1_STRING_type(x) ASN1_STRING_type(x)
#define M_ASN1_STRING_data(x) ASN1_STRING_data(x)
#define M_ASN1_BIT_STRING_new() ASN1_BIT_STRING_new()
#define M_ASN1_BIT_STRING_free(a) ASN1_BIT_STRING_free(a)
#define M_ASN1_BIT_STRING_dup(a) ASN1_STRING_dup(a)
#define M_ASN1_BIT_STRING_cmp(a, b) ASN1_STRING_cmp(a, b)
#define M_ASN1_BIT_STRING_set(a, b, c) ASN1_BIT_STRING_set(a, b, c)
#define M_ASN1_INTEGER_new() ASN1_INTEGER_new()
#define M_ASN1_INTEGER_free(a) ASN1_INTEGER_free(a)
#define M_ASN1_INTEGER_dup(a) ASN1_INTEGER_dup(a)
#define M_ASN1_INTEGER_cmp(a, b) ASN1_INTEGER_cmp(a, b)
#define M_ASN1_ENUMERATED_new() ASN1_ENUMERATED_new()
#define M_ASN1_ENUMERATED_free(a) ASN1_ENUMERATED_free(a)
#define M_ASN1_ENUMERATED_dup(a) ASN1_STRING_dup(a)
#define M_ASN1_ENUMERATED_cmp(a, b) ASN1_STRING_cmp(a, b)
#define M_ASN1_OCTET_STRING_new() ASN1_OCTET_STRING_new()
#define M_ASN1_OCTET_STRING_free(a) ASN1_OCTET_STRING_free()
#define M_ASN1_OCTET_STRING_dup(a) ASN1_OCTET_STRING_dup(a)
#define M_ASN1_OCTET_STRING_cmp(a, b) ASN1_OCTET_STRING_cmp(a, b)
#define M_ASN1_OCTET_STRING_set(a, b, c) ASN1_OCTET_STRING_set(a, b, c)
#define M_ASN1_OCTET_STRING_print(a, b) ASN1_STRING_print(a, b)
#define M_ASN1_PRINTABLESTRING_new() ASN1_PRINTABLESTRING_new()
#define M_ASN1_PRINTABLESTRING_free(a) ASN1_PRINTABLESTRING_free(a)
#define M_ASN1_IA5STRING_new() ASN1_IA5STRING_new()
#define M_ASN1_IA5STRING_free(a) ASN1_IA5STRING_free(a)
#define M_ASN1_IA5STRING_dup(a) ASN1_STRING_dup(a)
#define M_ASN1_UTCTIME_new() ASN1_UTCTIME_new()
#define M_ASN1_UTCTIME_free(a) ASN1_UTCTIME_free(a)
#define M_ASN1_UTCTIME_dup(a) ASN1_STRING_dup(a)
#define M_ASN1_T61STRING_new() ASN1_T61STRING_new()
#define M_ASN1_T61STRING_free(a) ASN1_T61STRING_free(a)
#define M_ASN1_GENERALIZEDTIME_new() ASN1_GENERALIZEDTIME_new()
#define M_ASN1_GENERALIZEDTIME_free(a) ASN1_GENERALIZEDTIME_free(a)
#define M_ASN1_GENERALIZEDTIME_dup(a) ASN1_STRING_dup(a)
#define M_ASN1_GENERALSTRING_new() ASN1_GENERALSTRING_new()
#define M_ASN1_GENERALSTRING_free(a) ASN1_GENERALSTRING_free(a)
#define M_ASN1_UNIVERSALSTRING_new() ASN1_UNIVERSALSTRING_new()
#define M_ASN1_UNIVERSALSTRING_free(a) ASN1_UNIVERSALSTRING_free(a)
#define M_ASN1_BMPSTRING_new() ASN1_BMPSTRING_new()
#define M_ASN1_BMPSTRING_free(a) ASN1_BMPSTRING_free(a)
#define M_ASN1_VISIBLESTRING_new() ASN1_VISIBLESTRING_new()
#define M_ASN1_VISIBLESTRING_free(a) ASN1_VISIBLESTRING_free(a)
#define M_ASN1_UTF8STRING_new() ASN1_UTF8STRING_new()
#define M_ASN1_UTF8STRING_free(a) ASN1_UTF8STRING_free(a)

// B_ASN1_PRINTABLE is a bitmask for an ad-hoc subset of string-like types. Note
// the presence of |B_ASN1_UNKNOWN| means it includes types which |ASN1_tag2bit|
// maps to |B_ASN1_UNKNOWN|.
//
// Do not use this. Despite the name, it has no connection to PrintableString or
// printable characters. See https://crbug.com/boringssl/412.
#define B_ASN1_PRINTABLE                                              \
  (B_ASN1_NUMERICSTRING | B_ASN1_PRINTABLESTRING | B_ASN1_T61STRING | \
   B_ASN1_IA5STRING | B_ASN1_BIT_STRING | B_ASN1_UNIVERSALSTRING |    \
   B_ASN1_BMPSTRING | B_ASN1_UTF8STRING | B_ASN1_SEQUENCE | B_ASN1_UNKNOWN)

// ASN1_PRINTABLE_new returns a newly-allocated |ASN1_STRING| with type -1, or
// NULL on error. The resulting |ASN1_STRING| is not a valid ASN.1 value until
// initialized with a value.
OPENSSL_EXPORT ASN1_STRING *ASN1_PRINTABLE_new(void);

// ASN1_PRINTABLE_free calls |ASN1_STRING_free|.
OPENSSL_EXPORT void ASN1_PRINTABLE_free(ASN1_STRING *str);

// d2i_ASN1_PRINTABLE parses up to |len| bytes from |*inp| as a DER-encoded
// CHOICE of an ad-hoc subset of string-like types, as described in
// |d2i_SAMPLE_with_reuse|.
//
// Do not use this. Despite, the name it has no connection to PrintableString or
// printable characters. See https://crbug.com/boringssl/412.
//
// TODO(https://crbug.com/boringssl/354): This function currently also accepts
// BER, but this will be removed in the future.
OPENSSL_EXPORT ASN1_STRING *d2i_ASN1_PRINTABLE(ASN1_STRING **out,
                                               const uint8_t **inp, long len);

// i2d_ASN1_PRINTABLE marshals |in| as DER, as described in |i2d_SAMPLE|.
//
// Do not use this. Despite the name, it has no connection to PrintableString or
// printable characters. See https://crbug.com/boringssl/412.
OPENSSL_EXPORT int i2d_ASN1_PRINTABLE(const ASN1_STRING *in, uint8_t **outp);

// ASN1_PRINTABLE is an |ASN1_ITEM| whose ASN.1 type is a CHOICE of an ad-hoc
// subset of string-like types, and whose C type is |ASN1_STRING*|.
//
// Do not use this. Despite the name, it has no connection to PrintableString or
// printable characters. See https://crbug.com/boringssl/412.
DECLARE_ASN1_ITEM(ASN1_PRINTABLE)

// ASN1_INTEGER_set sets |a| to an INTEGER with value |v|. It returns one on
// success and zero on error.
//
// Use |ASN1_INTEGER_set_uint64| and |ASN1_INTEGER_set_int64| instead.
OPENSSL_EXPORT int ASN1_INTEGER_set(ASN1_INTEGER *a, long v);

// ASN1_ENUMERATED_set sets |a| to an ENUMERATED with value |v|. It returns one
// on success and zero on error.
//
// Use |ASN1_ENUMERATED_set_uint64| and |ASN1_ENUMERATED_set_int64| instead.
OPENSSL_EXPORT int ASN1_ENUMERATED_set(ASN1_ENUMERATED *a, long v);

// ASN1_INTEGER_get returns the value of |a| as a |long|, or -1 if |a| is out of
// range or the wrong type.
//
// WARNING: This function's return value cannot distinguish errors from -1.
// Use |ASN1_INTEGER_get_uint64| and |ASN1_INTEGER_get_int64| instead.
OPENSSL_EXPORT long ASN1_INTEGER_get(const ASN1_INTEGER *a);

// ASN1_ENUMERATED_get returns the value of |a| as a |long|, or -1 if |a| is out
// of range or the wrong type.
//
// WARNING: This function's return value cannot distinguish errors from -1.
// Use |ASN1_ENUMERATED_get_uint64| and |ASN1_ENUMERATED_get_int64| instead.
OPENSSL_EXPORT long ASN1_ENUMERATED_get(const ASN1_ENUMERATED *a);


// General No-op Functions [Deprecated].

// ASN1_STRING_set_default_mask does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void ASN1_STRING_set_default_mask(
    unsigned long mask);

// ASN1_STRING_set_default_mask_asc returns one.
OPENSSL_EXPORT OPENSSL_DEPRECATED int ASN1_STRING_set_default_mask_asc(
    const char *p);

// ASN1_STRING_get_default_mask returns |B_ASN1_UTF8STRING|. This is
// the value AWS-LC uses by default and is not configurable.
OPENSSL_EXPORT OPENSSL_DEPRECATED unsigned long ASN1_STRING_get_default_mask(
    void);

// ASN1_STRING_TABLE_cleanup does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void ASN1_STRING_TABLE_cleanup(void);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(ASN1_OBJECT, ASN1_OBJECT_free)
BORINGSSL_MAKE_DELETER(ASN1_STRING, ASN1_STRING_free)
BORINGSSL_MAKE_DELETER(ASN1_TYPE, ASN1_TYPE_free)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#define ASN1_R_ASN1_LENGTH_MISMATCH 100
#define ASN1_R_AUX_ERROR 101
#define ASN1_R_BAD_GET_ASN1_OBJECT_CALL 102
#define ASN1_R_BAD_OBJECT_HEADER 103
#define ASN1_R_BMPSTRING_IS_WRONG_LENGTH 104
#define ASN1_R_BN_LIB 105
#define ASN1_R_BOOLEAN_IS_WRONG_LENGTH 106
#define ASN1_R_BUFFER_TOO_SMALL 107
#define ASN1_R_CONTEXT_NOT_INITIALISED 108
#define ASN1_R_DECODE_ERROR 109
#define ASN1_R_DEPTH_EXCEEDED 110
#define ASN1_R_DIGEST_AND_KEY_TYPE_NOT_SUPPORTED 111
#define ASN1_R_ENCODE_ERROR 112
#define ASN1_R_ERROR_GETTING_TIME 113
#define ASN1_R_EXPECTING_AN_ASN1_SEQUENCE 114
#define ASN1_R_EXPECTING_AN_INTEGER 115
#define ASN1_R_EXPECTING_AN_OBJECT 116
#define ASN1_R_EXPECTING_A_BOOLEAN 117
#define ASN1_R_EXPECTING_A_TIME 118
#define ASN1_R_EXPLICIT_LENGTH_MISMATCH 119
#define ASN1_R_EXPLICIT_TAG_NOT_CONSTRUCTED 120
#define ASN1_R_FIELD_MISSING 121
#define ASN1_R_FIRST_NUM_TOO_LARGE 122
#define ASN1_R_HEADER_TOO_LONG 123
#define ASN1_R_ILLEGAL_BITSTRING_FORMAT 124
#define ASN1_R_ILLEGAL_BOOLEAN 125
#define ASN1_R_ILLEGAL_CHARACTERS 126
#define ASN1_R_ILLEGAL_FORMAT 127
#define ASN1_R_ILLEGAL_HEX 128
#define ASN1_R_ILLEGAL_IMPLICIT_TAG 129
#define ASN1_R_ILLEGAL_INTEGER 130
#define ASN1_R_ILLEGAL_NESTED_TAGGING 131
#define ASN1_R_ILLEGAL_NULL 132
#define ASN1_R_ILLEGAL_NULL_VALUE 133
#define ASN1_R_ILLEGAL_OBJECT 134
#define ASN1_R_ILLEGAL_OPTIONAL_ANY 135
#define ASN1_R_ILLEGAL_OPTIONS_ON_ITEM_TEMPLATE 136
#define ASN1_R_ILLEGAL_TAGGED_ANY 137
#define ASN1_R_ILLEGAL_TIME_VALUE 138
#define ASN1_R_INTEGER_NOT_ASCII_FORMAT 139
#define ASN1_R_INTEGER_TOO_LARGE_FOR_LONG 140
#define ASN1_R_INVALID_BIT_STRING_BITS_LEFT 141
#define ASN1_R_INVALID_BMPSTRING 142
#define ASN1_R_INVALID_DIGIT 143
#define ASN1_R_INVALID_MODIFIER 144
#define ASN1_R_INVALID_NUMBER 145
#define ASN1_R_INVALID_OBJECT_ENCODING 146
#define ASN1_R_INVALID_SEPARATOR 147
#define ASN1_R_INVALID_TIME_FORMAT 148
#define ASN1_R_INVALID_UNIVERSALSTRING 149
#define ASN1_R_INVALID_UTF8STRING 150
#define ASN1_R_LIST_ERROR 151
#define ASN1_R_MISSING_ASN1_EOS 152
#define ASN1_R_MISSING_EOC 153
#define ASN1_R_MISSING_SECOND_NUMBER 154
#define ASN1_R_MISSING_VALUE 155
#define ASN1_R_MSTRING_NOT_UNIVERSAL 156
#define ASN1_R_MSTRING_WRONG_TAG 157
#define ASN1_R_NESTED_ASN1_ERROR 158
#define ASN1_R_NESTED_ASN1_STRING 159
#define ASN1_R_NON_HEX_CHARACTERS 160
#define ASN1_R_NOT_ASCII_FORMAT 161
#define ASN1_R_NOT_ENOUGH_DATA 162
#define ASN1_R_NO_MATCHING_CHOICE_TYPE 163
#define ASN1_R_NULL_IS_WRONG_LENGTH 164
#define ASN1_R_OBJECT_NOT_ASCII_FORMAT 165
#define ASN1_R_ODD_NUMBER_OF_CHARS 166
#define ASN1_R_SECOND_NUMBER_TOO_LARGE 167
#define ASN1_R_SEQUENCE_LENGTH_MISMATCH 168
#define ASN1_R_SEQUENCE_NOT_CONSTRUCTED 169
#define ASN1_R_SEQUENCE_OR_SET_NEEDS_CONFIG 170
#define ASN1_R_SHORT_LINE 171
#define ASN1_R_STREAMING_NOT_SUPPORTED 172
#define ASN1_R_STRING_TOO_LONG 173
#define ASN1_R_STRING_TOO_SHORT 174
#define ASN1_R_TAG_VALUE_TOO_HIGH 175
#define ASN1_R_TIME_NOT_ASCII_FORMAT 176
#define ASN1_R_TOO_LONG 177
#define ASN1_R_TYPE_NOT_CONSTRUCTED 178
#define ASN1_R_TYPE_NOT_PRIMITIVE 179
#define ASN1_R_UNEXPECTED_EOC 180
#define ASN1_R_UNIVERSALSTRING_IS_WRONG_LENGTH 181
#define ASN1_R_UNKNOWN_FORMAT 182
#define ASN1_R_UNKNOWN_MESSAGE_DIGEST_ALGORITHM 183
#define ASN1_R_UNKNOWN_SIGNATURE_ALGORITHM 184
#define ASN1_R_UNKNOWN_TAG 185
#define ASN1_R_UNSUPPORTED_ANY_DEFINED_BY_TYPE 186
#define ASN1_R_UNSUPPORTED_PUBLIC_KEY_TYPE 187
#define ASN1_R_UNSUPPORTED_TYPE 188
#define ASN1_R_WRONG_PUBLIC_KEY_TYPE 189
#define ASN1_R_WRONG_TAG 190
#define ASN1_R_WRONG_TYPE 191
#define ASN1_R_NESTED_TOO_DEEP 192
#define ASN1_R_BAD_TEMPLATE 193
#define ASN1_R_INVALID_BIT_STRING_PADDING 194
#define ASN1_R_WRONG_INTEGER_TYPE 195
#define ASN1_R_INVALID_INTEGER 196

#endif  // OPENSSL_HEADER_ASN1_H
