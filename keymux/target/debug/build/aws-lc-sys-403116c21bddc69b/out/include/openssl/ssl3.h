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
/* ====================================================================
 * Copyright (c) 1998-2002 The OpenSSL Project.  All rights reserved.
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
 * ECC cipher suite support in OpenSSL originally developed by
 * SUN MICROSYSTEMS, INC., and contributed to the OpenSSL project.
 */

#ifndef OPENSSL_HEADER_SSL3_H
#define OPENSSL_HEADER_SSL3_H

#include <openssl/aead.h>
#include <openssl/type_check.h>

#ifdef  __cplusplus
extern "C" {
#endif


// These are kept to support clients that negotiates higher protocol versions
// using SSLv2 client hello records.
#define SSL2_MT_CLIENT_HELLO 1
#define SSL2_VERSION 0x0002

// Signalling cipher suite value from RFC 5746.
#define SSL3_CK_SCSV 0x030000FF
// Fallback signalling cipher suite value from RFC 7507.
#define SSL3_CK_FALLBACK_SCSV 0x03005600

#define SSL3_CK_RSA_NULL_MD5 0x03000001
#define SSL3_CK_RSA_NULL_SHA 0x03000002
#define SSL3_CK_RSA_RC4_40_MD5 0x03000003
#define SSL3_CK_RSA_RC4_128_MD5 0x03000004
#define SSL3_CK_RSA_RC4_128_SHA 0x03000005
#define SSL3_CK_RSA_RC2_40_MD5 0x03000006
#define SSL3_CK_RSA_IDEA_128_SHA 0x03000007
#define SSL3_CK_RSA_DES_40_CBC_SHA 0x03000008
#define SSL3_CK_RSA_DES_64_CBC_SHA 0x03000009
#define SSL3_CK_RSA_DES_192_CBC3_SHA 0x0300000A

#define SSL3_CK_DH_DSS_DES_40_CBC_SHA 0x0300000B
#define SSL3_CK_DH_DSS_DES_64_CBC_SHA 0x0300000C
#define SSL3_CK_DH_DSS_DES_192_CBC3_SHA 0x0300000D
#define SSL3_CK_DH_RSA_DES_40_CBC_SHA 0x0300000E
#define SSL3_CK_DH_RSA_DES_64_CBC_SHA 0x0300000F
#define SSL3_CK_DH_RSA_DES_192_CBC3_SHA 0x03000010

#define SSL3_CK_EDH_DSS_DES_40_CBC_SHA 0x03000011
#define SSL3_CK_EDH_DSS_DES_64_CBC_SHA 0x03000012
#define SSL3_CK_EDH_DSS_DES_192_CBC3_SHA 0x03000013
#define SSL3_CK_EDH_RSA_DES_40_CBC_SHA 0x03000014
#define SSL3_CK_EDH_RSA_DES_64_CBC_SHA 0x03000015
#define SSL3_CK_EDH_RSA_DES_192_CBC3_SHA 0x03000016

#define SSL3_CK_ADH_RC4_40_MD5 0x03000017
#define SSL3_CK_ADH_RC4_128_MD5 0x03000018
#define SSL3_CK_ADH_DES_40_CBC_SHA 0x03000019
#define SSL3_CK_ADH_DES_64_CBC_SHA 0x0300001A
#define SSL3_CK_ADH_DES_192_CBC_SHA 0x0300001B

#define SSL3_TXT_RSA_NULL_MD5 "NULL-MD5"
#define SSL3_TXT_RSA_NULL_SHA "NULL-SHA"
#define SSL3_TXT_RSA_RC4_40_MD5 "EXP-RC4-MD5"
#define SSL3_TXT_RSA_RC4_128_MD5 "RC4-MD5"
#define SSL3_TXT_RSA_RC4_128_SHA "RC4-SHA"
#define SSL3_TXT_RSA_RC2_40_MD5 "EXP-RC2-CBC-MD5"
#define SSL3_TXT_RSA_IDEA_128_SHA "IDEA-CBC-SHA"
#define SSL3_TXT_RSA_DES_40_CBC_SHA "EXP-DES-CBC-SHA"
#define SSL3_TXT_RSA_DES_64_CBC_SHA "DES-CBC-SHA"
#define SSL3_TXT_RSA_DES_192_CBC3_SHA "DES-CBC3-SHA"

#define SSL3_TXT_DH_DSS_DES_40_CBC_SHA "EXP-DH-DSS-DES-CBC-SHA"
#define SSL3_TXT_DH_DSS_DES_64_CBC_SHA "DH-DSS-DES-CBC-SHA"
#define SSL3_TXT_DH_DSS_DES_192_CBC3_SHA "DH-DSS-DES-CBC3-SHA"
#define SSL3_TXT_DH_RSA_DES_40_CBC_SHA "EXP-DH-RSA-DES-CBC-SHA"
#define SSL3_TXT_DH_RSA_DES_64_CBC_SHA "DH-RSA-DES-CBC-SHA"
#define SSL3_TXT_DH_RSA_DES_192_CBC3_SHA "DH-RSA-DES-CBC3-SHA"

#define SSL3_TXT_EDH_DSS_DES_40_CBC_SHA "EXP-EDH-DSS-DES-CBC-SHA"
#define SSL3_TXT_EDH_DSS_DES_64_CBC_SHA "EDH-DSS-DES-CBC-SHA"
#define SSL3_TXT_EDH_DSS_DES_192_CBC3_SHA "EDH-DSS-DES-CBC3-SHA"
#define SSL3_TXT_EDH_RSA_DES_40_CBC_SHA "EXP-EDH-RSA-DES-CBC-SHA"
#define SSL3_TXT_EDH_RSA_DES_64_CBC_SHA "EDH-RSA-DES-CBC-SHA"
#define SSL3_TXT_EDH_RSA_DES_192_CBC3_SHA "EDH-RSA-DES-CBC3-SHA"

#define SSL3_TXT_ADH_RC4_40_MD5 "EXP-ADH-RC4-MD5"
#define SSL3_TXT_ADH_RC4_128_MD5 "ADH-RC4-MD5"
#define SSL3_TXT_ADH_DES_40_CBC_SHA "EXP-ADH-DES-CBC-SHA"
#define SSL3_TXT_ADH_DES_64_CBC_SHA "ADH-DES-CBC-SHA"
#define SSL3_TXT_ADH_DES_192_CBC_SHA "ADH-DES-CBC3-SHA"

#define SSL3_SSL_SESSION_ID_LENGTH 32
#define SSL3_MAX_SSL_SESSION_ID_LENGTH 32

#define SSL3_MASTER_SECRET_SIZE 48
#define SSL3_RANDOM_SIZE 32
#define SSL3_SESSION_ID_SIZE 32
#define SSL3_RT_HEADER_LENGTH 5

#define SSL3_HM_HEADER_LENGTH 4

#ifndef SSL3_ALIGN_PAYLOAD
// Some will argue that this increases memory footprint, but it's not actually
// true. Point is that malloc has to return at least 64-bit aligned pointers,
// meaning that allocating 5 bytes wastes 3 bytes in either case. Suggested
// pre-gaping simply moves these wasted bytes from the end of allocated region
// to its front, but makes data payload aligned, which improves performance.
#define SSL3_ALIGN_PAYLOAD 8
#else
#if (SSL3_ALIGN_PAYLOAD & (SSL3_ALIGN_PAYLOAD - 1)) != 0
#error "insane SSL3_ALIGN_PAYLOAD"
#undef SSL3_ALIGN_PAYLOAD
#endif
#endif

// This is the maximum MAC (digest) size used by the SSL library. Currently
// maximum of 20 is used by SHA1, but we reserve for future extension for
// 512-bit hashes.

#define SSL3_RT_MAX_MD_SIZE 64

// Maximum block size used in all ciphersuites. Currently 16 for AES.

#define SSL_RT_MAX_CIPHER_BLOCK_SIZE 16

// Maximum plaintext length: defined by SSL/TLS standards
#define SSL3_RT_MAX_PLAIN_LENGTH 16384
// Maximum compression overhead: defined by SSL/TLS standards
#define SSL3_RT_MAX_COMPRESSED_OVERHEAD 1024

// The standards give a maximum encryption overhead of 1024 bytes. In practice
// the value is lower than this. The overhead is the maximum number of padding
// bytes (256) plus the mac size.
//
// TODO(davidben): This derivation doesn't take AEADs into account, or TLS 1.1
// explicit nonces. It happens to work because |SSL3_RT_MAX_MD_SIZE| is larger
// than necessary and no true AEAD has variable overhead in TLS 1.2.
#define SSL3_RT_MAX_ENCRYPTED_OVERHEAD (256 + SSL3_RT_MAX_MD_SIZE)

// SSL3_RT_SEND_MAX_ENCRYPTED_OVERHEAD is the maximum overhead in encrypting a
// record. This does not include the record header. Some ciphers use explicit
// nonces, so it includes both the AEAD overhead as well as the nonce.
#define SSL3_RT_SEND_MAX_ENCRYPTED_OVERHEAD \
    (EVP_AEAD_MAX_OVERHEAD + EVP_AEAD_MAX_NONCE_LENGTH)

OPENSSL_STATIC_ASSERT(SSL3_RT_MAX_ENCRYPTED_OVERHEAD >=
                          SSL3_RT_SEND_MAX_ENCRYPTED_OVERHEAD,
                      max_overheads_are_inconsistent)

// SSL3_RT_MAX_COMPRESSED_LENGTH is an alias for
// |SSL3_RT_MAX_PLAIN_LENGTH|. Compression is gone, so don't include the
// compression overhead.
#define SSL3_RT_MAX_COMPRESSED_LENGTH SSL3_RT_MAX_PLAIN_LENGTH

#define SSL3_RT_MAX_ENCRYPTED_LENGTH \
  (SSL3_RT_MAX_ENCRYPTED_OVERHEAD + SSL3_RT_MAX_COMPRESSED_LENGTH)
#define SSL3_RT_MAX_PACKET_SIZE \
  (SSL3_RT_MAX_ENCRYPTED_LENGTH + SSL3_RT_HEADER_LENGTH)

#define SSL3_MD_CLIENT_FINISHED_CONST "\x43\x4C\x4E\x54"
#define SSL3_MD_SERVER_FINISHED_CONST "\x53\x52\x56\x52"

#define SSL3_RT_CHANGE_CIPHER_SPEC 20
#define SSL3_RT_ALERT 21
#define SSL3_RT_HANDSHAKE 22
#define SSL3_RT_APPLICATION_DATA 23

// Pseudo content type for SSL/TLS header info
#define SSL3_RT_HEADER 0x100
#define SSL3_RT_CLIENT_HELLO_INNER 0x101

#define SSL3_AL_WARNING 1
#define SSL3_AL_FATAL 2

#define SSL3_AD_CLOSE_NOTIFY 0
#define SSL3_AD_UNEXPECTED_MESSAGE 10     // fatal
#define SSL3_AD_BAD_RECORD_MAC 20         // fatal
#define SSL3_AD_DECOMPRESSION_FAILURE 30  // fatal
#define SSL3_AD_HANDSHAKE_FAILURE 40      // fatal
#define SSL3_AD_NO_CERTIFICATE 41
#define SSL3_AD_BAD_CERTIFICATE 42
#define SSL3_AD_UNSUPPORTED_CERTIFICATE 43
#define SSL3_AD_CERTIFICATE_REVOKED 44
#define SSL3_AD_CERTIFICATE_EXPIRED 45
#define SSL3_AD_CERTIFICATE_UNKNOWN 46
#define SSL3_AD_ILLEGAL_PARAMETER 47       // fatal
#define SSL3_AD_INAPPROPRIATE_FALLBACK 86  // fatal

#define SSL3_CT_RSA_SIGN 1

#define SSL3_MT_HELLO_REQUEST 0
#define SSL3_MT_CLIENT_HELLO 1
#define SSL3_MT_SERVER_HELLO 2
#define SSL3_MT_NEW_SESSION_TICKET 4
#define SSL3_MT_END_OF_EARLY_DATA 5
#define SSL3_MT_ENCRYPTED_EXTENSIONS 8
#define SSL3_MT_CERTIFICATE 11
#define SSL3_MT_SERVER_KEY_EXCHANGE 12
#define SSL3_MT_CERTIFICATE_REQUEST 13
#define SSL3_MT_SERVER_HELLO_DONE 14
#define SSL3_MT_CERTIFICATE_VERIFY 15
#define SSL3_MT_CLIENT_KEY_EXCHANGE 16
#define SSL3_MT_FINISHED 20
#define SSL3_MT_CERTIFICATE_STATUS 22
#define SSL3_MT_SUPPLEMENTAL_DATA 23
#define SSL3_MT_KEY_UPDATE 24
#define SSL3_MT_COMPRESSED_CERTIFICATE 25
#define SSL3_MT_NEXT_PROTO 67
#define SSL3_MT_CHANNEL_ID 203
#define SSL3_MT_MESSAGE_HASH 254
#define DTLS1_MT_HELLO_VERIFY_REQUEST 3

// The following are legacy aliases for consumers which use
// |SSL_CTX_set_msg_callback|.
#define SSL3_MT_SERVER_DONE SSL3_MT_SERVER_HELLO_DONE
#define SSL3_MT_NEWSESSION_TICKET SSL3_MT_NEW_SESSION_TICKET


#define SSL3_MT_CCS 1


#ifdef  __cplusplus
}  // extern C
#endif

#endif  // OPENSSL_HEADER_SSL3_H
