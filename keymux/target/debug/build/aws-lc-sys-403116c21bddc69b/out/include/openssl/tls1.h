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
 * Copyright (c) 1998-2006 The OpenSSL Project.  All rights reserved.
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
 * ECC cipher suite support in OpenSSL originally written by
 * Vipul Gupta and Sumit Gupta of Sun Microsystems Laboratories.
 *
 */
/* ====================================================================
 * Copyright 2005 Nokia. All rights reserved.
 *
 * The portions of the attached software ("Contribution") is developed by
 * Nokia Corporation and is licensed pursuant to the OpenSSL open source
 * license.
 *
 * The Contribution, originally written by Mika Kousa and Pasi Eronen of
 * Nokia Corporation, consists of the "PSK" (Pre-Shared Key) ciphersuites
 * support (see RFC 4279) to OpenSSL.
 *
 * No patent licenses or other rights except those expressly stated in
 * the OpenSSL open source license shall be deemed granted or received
 * expressly, by implication, estoppel, or otherwise.
 *
 * No assurances are provided by Nokia that the Contribution does not
 * infringe the patent or other intellectual property rights of any third
 * party or that the license provides you with all the necessary rights
 * to make use of the Contribution.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" WITHOUT WARRANTY OF ANY KIND. IN
 * ADDITION TO THE DISCLAIMERS INCLUDED IN THE LICENSE, NOKIA
 * SPECIFICALLY DISCLAIMS ANY LIABILITY FOR CLAIMS BROUGHT BY YOU OR ANY
 * OTHER ENTITY BASED ON INFRINGEMENT OF INTELLECTUAL PROPERTY RIGHTS OR
 * OTHERWISE.
 */

#ifndef OPENSSL_HEADER_TLS1_H
#define OPENSSL_HEADER_TLS1_H

#include <openssl/base.h>

#ifdef  __cplusplus
extern "C" {
#endif


#define TLS1_AD_END_OF_EARLY_DATA 1
#define TLS1_AD_DECRYPTION_FAILED 21
#define TLS1_AD_RECORD_OVERFLOW 22
#define TLS1_AD_UNKNOWN_CA 48
#define TLS1_AD_ACCESS_DENIED 49
#define TLS1_AD_DECODE_ERROR 50
#define TLS1_AD_DECRYPT_ERROR 51
#define TLS1_AD_EXPORT_RESTRICTION 60
#define TLS1_AD_PROTOCOL_VERSION 70
#define TLS1_AD_INSUFFICIENT_SECURITY 71
#define TLS1_AD_INTERNAL_ERROR 80
#define TLS1_AD_USER_CANCELLED 90
#define TLS1_AD_NO_RENEGOTIATION 100
#define TLS1_AD_MISSING_EXTENSION 109
#define TLS1_AD_UNSUPPORTED_EXTENSION 110
#define TLS1_AD_CERTIFICATE_UNOBTAINABLE 111
#define TLS1_AD_UNRECOGNIZED_NAME 112
#define TLS1_AD_BAD_CERTIFICATE_STATUS_RESPONSE 113
#define TLS1_AD_BAD_CERTIFICATE_HASH_VALUE 114
#define TLS1_AD_UNKNOWN_PSK_IDENTITY 115
#define TLS1_AD_CERTIFICATE_REQUIRED 116
#define TLS1_AD_NO_APPLICATION_PROTOCOL 120
#define TLS1_AD_ECH_REQUIRED 121  // draft-ietf-tls-esni-13

// ExtensionType values from RFC 6066
#define TLSEXT_TYPE_server_name 0
#define TLSEXT_TYPE_status_request 5

// ExtensionType values from RFC 4492
#define TLSEXT_TYPE_ec_point_formats 11

// ExtensionType values from RFC 5246
#define TLSEXT_TYPE_signature_algorithms 13

// ExtensionType value from RFC 5764
#define TLSEXT_TYPE_srtp 14

// ExtensionType value from RFC 7301
#define TLSEXT_TYPE_application_layer_protocol_negotiation 16

// ExtensionType value from RFC 7685
#define TLSEXT_TYPE_padding 21

// ExtensionType value from RFC 7627
#define TLSEXT_TYPE_extended_master_secret 23

// ExtensionType value from draft-ietf-quic-tls. Drafts 00 through 32 use
// 0xffa5 which is part of the Private Use section of the registry, and it
// collides with TLS-LTS and, based on scans, something else too (though this
// hasn't been a problem in practice since it's QUIC-only). Drafts 33 onward
// use the value 57 which was officially registered with IANA.
#define TLSEXT_TYPE_quic_transport_parameters_legacy 0xffa5

// ExtensionType value from RFC 9000
#define TLSEXT_TYPE_quic_transport_parameters 57

// TLSEXT_TYPE_quic_transport_parameters_standard is an alias for
// |TLSEXT_TYPE_quic_transport_parameters|. Use
// |TLSEXT_TYPE_quic_transport_parameters| instead.
#define TLSEXT_TYPE_quic_transport_parameters_standard \
  TLSEXT_TYPE_quic_transport_parameters

// ExtensionType value from RFC 8879
#define TLSEXT_TYPE_cert_compression 27

// ExtensionType value from RFC 4507
#define TLSEXT_TYPE_session_ticket 35

// ExtensionType values from RFC 8446
#define TLSEXT_TYPE_supported_groups 10
#define TLSEXT_TYPE_pre_shared_key 41
#define TLSEXT_TYPE_early_data 42
#define TLSEXT_TYPE_supported_versions 43
#define TLSEXT_TYPE_cookie 44
#define TLSEXT_TYPE_psk_key_exchange_modes 45
#define TLSEXT_TYPE_certificate_authorities 47
#define TLSEXT_TYPE_signature_algorithms_cert 50
#define TLSEXT_TYPE_key_share 51

// ExtensionType value from RFC 5746
#define TLSEXT_TYPE_renegotiate 0xff01

// ExtensionType value from RFC 9345
#define TLSEXT_TYPE_delegated_credential 34

// ExtensionType value from draft-vvv-tls-alps. This is not an IANA defined
// extension number.
#define TLSEXT_TYPE_application_settings_old 17513
#define TLSEXT_TYPE_application_settings 17613

// ExtensionType values from draft-ietf-tls-esni-13. This is not an IANA defined
// extension number.
#define TLSEXT_TYPE_encrypted_client_hello 0xfe0d
#define TLSEXT_TYPE_ech_outer_extensions 0xfd00

// ExtensionType value from RFC 6962
#define TLSEXT_TYPE_certificate_timestamp 18

// This is not an IANA defined extension number
#define TLSEXT_TYPE_next_proto_neg 13172

// This is not an IANA defined extension number
#define TLSEXT_TYPE_channel_id 30032

// status request value from RFC 3546
#define TLSEXT_STATUSTYPE_nothing (-1)
#define TLSEXT_STATUSTYPE_ocsp 1

// ECPointFormat values from RFC 4492
#define TLSEXT_ECPOINTFORMAT_uncompressed 0
#define TLSEXT_ECPOINTFORMAT_ansiX962_compressed_prime 1

// Signature and hash algorithms from RFC 5246

#define TLSEXT_signature_anonymous 0
#define TLSEXT_signature_rsa 1
#define TLSEXT_signature_dsa 2
#define TLSEXT_signature_ecdsa 3

#define TLSEXT_hash_none 0
#define TLSEXT_hash_md5 1
#define TLSEXT_hash_sha1 2
#define TLSEXT_hash_sha224 3
#define TLSEXT_hash_sha256 4
#define TLSEXT_hash_sha384 5
#define TLSEXT_hash_sha512 6

// From https://www.rfc-editor.org/rfc/rfc8879.html#section-3
#define TLSEXT_cert_compression_zlib 1
#define TLSEXT_cert_compression_brotli 2

#define TLSEXT_MAXLEN_host_name 255

// PSK ciphersuites from 4279
#define TLS1_CK_PSK_WITH_RC4_128_SHA                    0x0300008A
#define TLS1_CK_PSK_WITH_3DES_EDE_CBC_SHA               0x0300008B
#define TLS1_CK_PSK_WITH_AES_128_CBC_SHA                0x0300008C
#define TLS1_CK_PSK_WITH_AES_256_CBC_SHA                0x0300008D

// PSK ciphersuites from RFC 5489
#define TLS1_CK_ECDHE_PSK_WITH_AES_128_CBC_SHA          0x0300C035
#define TLS1_CK_ECDHE_PSK_WITH_AES_256_CBC_SHA          0x0300C036

// Additional TLS ciphersuites from expired Internet Draft
// draft-ietf-tls-56-bit-ciphersuites-01.txt
// (available if TLS1_ALLOW_EXPERIMENTAL_CIPHERSUITES is defined, see
// s3_lib.c).  We actually treat them like SSL 3.0 ciphers, which we probably
// shouldn't.  Note that the first two are actually not in the IDs.
#define TLS1_CK_RSA_EXPORT1024_WITH_RC4_56_MD5 0x03000060      // not in ID
#define TLS1_CK_RSA_EXPORT1024_WITH_RC2_CBC_56_MD5 0x03000061  // not in ID
#define TLS1_CK_RSA_EXPORT1024_WITH_DES_CBC_SHA 0x03000062
#define TLS1_CK_DHE_DSS_EXPORT1024_WITH_DES_CBC_SHA 0x03000063
#define TLS1_CK_RSA_EXPORT1024_WITH_RC4_56_SHA 0x03000064
#define TLS1_CK_DHE_DSS_EXPORT1024_WITH_RC4_56_SHA 0x03000065
#define TLS1_CK_DHE_DSS_WITH_RC4_128_SHA 0x03000066

// AES ciphersuites from RFC 3268

#define TLS1_CK_RSA_WITH_AES_128_SHA 0x0300002F
#define TLS1_CK_DH_DSS_WITH_AES_128_SHA 0x03000030
#define TLS1_CK_DH_RSA_WITH_AES_128_SHA 0x03000031
#define TLS1_CK_DHE_DSS_WITH_AES_128_SHA 0x03000032
#define TLS1_CK_DHE_RSA_WITH_AES_128_SHA 0x03000033
#define TLS1_CK_ADH_WITH_AES_128_SHA 0x03000034

#define TLS1_CK_RSA_WITH_AES_256_SHA 0x03000035
#define TLS1_CK_DH_DSS_WITH_AES_256_SHA 0x03000036
#define TLS1_CK_DH_RSA_WITH_AES_256_SHA 0x03000037
#define TLS1_CK_DHE_DSS_WITH_AES_256_SHA 0x03000038
#define TLS1_CK_DHE_RSA_WITH_AES_256_SHA 0x03000039
#define TLS1_CK_ADH_WITH_AES_256_SHA 0x0300003A

// TLS v1.2 ciphersuites
#define TLS1_CK_RSA_WITH_NULL_SHA256 0x0300003B
#define TLS1_CK_RSA_WITH_AES_128_SHA256 0x0300003C
#define TLS1_CK_RSA_WITH_AES_256_SHA256 0x0300003D
#define TLS1_CK_DH_DSS_WITH_AES_128_SHA256 0x0300003E
#define TLS1_CK_DH_RSA_WITH_AES_128_SHA256 0x0300003F
#define TLS1_CK_DHE_DSS_WITH_AES_128_SHA256 0x03000040

// Camellia ciphersuites from RFC 4132
#define TLS1_CK_RSA_WITH_CAMELLIA_128_CBC_SHA 0x03000041
#define TLS1_CK_DH_DSS_WITH_CAMELLIA_128_CBC_SHA 0x03000042
#define TLS1_CK_DH_RSA_WITH_CAMELLIA_128_CBC_SHA 0x03000043
#define TLS1_CK_DHE_DSS_WITH_CAMELLIA_128_CBC_SHA 0x03000044
#define TLS1_CK_DHE_RSA_WITH_CAMELLIA_128_CBC_SHA 0x03000045
#define TLS1_CK_ADH_WITH_CAMELLIA_128_CBC_SHA 0x03000046

// TLS v1.2 ciphersuites
#define TLS1_CK_DHE_RSA_WITH_AES_128_SHA256 0x03000067
#define TLS1_CK_DH_DSS_WITH_AES_256_SHA256 0x03000068
#define TLS1_CK_DH_RSA_WITH_AES_256_SHA256 0x03000069
#define TLS1_CK_DHE_DSS_WITH_AES_256_SHA256 0x0300006A
#define TLS1_CK_DHE_RSA_WITH_AES_256_SHA256 0x0300006B
#define TLS1_CK_ADH_WITH_AES_128_SHA256 0x0300006C
#define TLS1_CK_ADH_WITH_AES_256_SHA256 0x0300006D

// Camellia ciphersuites from RFC 4132
#define TLS1_CK_RSA_WITH_CAMELLIA_256_CBC_SHA 0x03000084
#define TLS1_CK_DH_DSS_WITH_CAMELLIA_256_CBC_SHA 0x03000085
#define TLS1_CK_DH_RSA_WITH_CAMELLIA_256_CBC_SHA 0x03000086
#define TLS1_CK_DHE_DSS_WITH_CAMELLIA_256_CBC_SHA 0x03000087
#define TLS1_CK_DHE_RSA_WITH_CAMELLIA_256_CBC_SHA 0x03000088
#define TLS1_CK_ADH_WITH_CAMELLIA_256_CBC_SHA 0x03000089

// SEED ciphersuites from RFC 4162
#define TLS1_CK_RSA_WITH_SEED_SHA 0x03000096
#define TLS1_CK_DH_DSS_WITH_SEED_SHA 0x03000097
#define TLS1_CK_DH_RSA_WITH_SEED_SHA 0x03000098
#define TLS1_CK_DHE_DSS_WITH_SEED_SHA 0x03000099
#define TLS1_CK_DHE_RSA_WITH_SEED_SHA 0x0300009A
#define TLS1_CK_ADH_WITH_SEED_SHA 0x0300009B

// TLS v1.2 GCM ciphersuites from RFC 5288
#define TLS1_CK_RSA_WITH_AES_128_GCM_SHA256 0x0300009C
#define TLS1_CK_RSA_WITH_AES_256_GCM_SHA384 0x0300009D
#define TLS1_CK_DHE_RSA_WITH_AES_128_GCM_SHA256 0x0300009E
#define TLS1_CK_DHE_RSA_WITH_AES_256_GCM_SHA384 0x0300009F
#define TLS1_CK_DH_RSA_WITH_AES_128_GCM_SHA256 0x030000A0
#define TLS1_CK_DH_RSA_WITH_AES_256_GCM_SHA384 0x030000A1
#define TLS1_CK_DHE_DSS_WITH_AES_128_GCM_SHA256 0x030000A2
#define TLS1_CK_DHE_DSS_WITH_AES_256_GCM_SHA384 0x030000A3
#define TLS1_CK_DH_DSS_WITH_AES_128_GCM_SHA256 0x030000A4
#define TLS1_CK_DH_DSS_WITH_AES_256_GCM_SHA384 0x030000A5
#define TLS1_CK_ADH_WITH_AES_128_GCM_SHA256 0x030000A6
#define TLS1_CK_ADH_WITH_AES_256_GCM_SHA384 0x030000A7

// ECC ciphersuites from RFC 4492
#define TLS1_CK_ECDH_ECDSA_WITH_NULL_SHA 0x0300C001
#define TLS1_CK_ECDH_ECDSA_WITH_RC4_128_SHA 0x0300C002
#define TLS1_CK_ECDH_ECDSA_WITH_DES_192_CBC3_SHA 0x0300C003
#define TLS1_CK_ECDH_ECDSA_WITH_AES_128_CBC_SHA 0x0300C004
#define TLS1_CK_ECDH_ECDSA_WITH_AES_256_CBC_SHA 0x0300C005

#define TLS1_CK_ECDHE_ECDSA_WITH_NULL_SHA 0x0300C006
#define TLS1_CK_ECDHE_ECDSA_WITH_RC4_128_SHA 0x0300C007
#define TLS1_CK_ECDHE_ECDSA_WITH_DES_192_CBC3_SHA 0x0300C008
#define TLS1_CK_ECDHE_ECDSA_WITH_AES_128_CBC_SHA 0x0300C009
#define TLS1_CK_ECDHE_ECDSA_WITH_AES_256_CBC_SHA 0x0300C00A

#define TLS1_CK_ECDH_RSA_WITH_NULL_SHA 0x0300C00B
#define TLS1_CK_ECDH_RSA_WITH_RC4_128_SHA 0x0300C00C
#define TLS1_CK_ECDH_RSA_WITH_DES_192_CBC3_SHA 0x0300C00D
#define TLS1_CK_ECDH_RSA_WITH_AES_128_CBC_SHA 0x0300C00E
#define TLS1_CK_ECDH_RSA_WITH_AES_256_CBC_SHA 0x0300C00F

#define TLS1_CK_ECDHE_RSA_WITH_NULL_SHA 0x0300C010
#define TLS1_CK_ECDHE_RSA_WITH_RC4_128_SHA 0x0300C011
#define TLS1_CK_ECDHE_RSA_WITH_DES_192_CBC3_SHA 0x0300C012
#define TLS1_CK_ECDHE_RSA_WITH_AES_128_CBC_SHA 0x0300C013
#define TLS1_CK_ECDHE_RSA_WITH_AES_256_CBC_SHA 0x0300C014

#define TLS1_CK_ECDH_anon_WITH_NULL_SHA 0x0300C015
#define TLS1_CK_ECDH_anon_WITH_RC4_128_SHA 0x0300C016
#define TLS1_CK_ECDH_anon_WITH_DES_192_CBC3_SHA 0x0300C017
#define TLS1_CK_ECDH_anon_WITH_AES_128_CBC_SHA 0x0300C018
#define TLS1_CK_ECDH_anon_WITH_AES_256_CBC_SHA 0x0300C019

// SRP ciphersuites from RFC 5054
#define TLS1_CK_SRP_SHA_WITH_3DES_EDE_CBC_SHA 0x0300C01A
#define TLS1_CK_SRP_SHA_RSA_WITH_3DES_EDE_CBC_SHA 0x0300C01B
#define TLS1_CK_SRP_SHA_DSS_WITH_3DES_EDE_CBC_SHA 0x0300C01C
#define TLS1_CK_SRP_SHA_WITH_AES_128_CBC_SHA 0x0300C01D
#define TLS1_CK_SRP_SHA_RSA_WITH_AES_128_CBC_SHA 0x0300C01E
#define TLS1_CK_SRP_SHA_DSS_WITH_AES_128_CBC_SHA 0x0300C01F
#define TLS1_CK_SRP_SHA_WITH_AES_256_CBC_SHA 0x0300C020
#define TLS1_CK_SRP_SHA_RSA_WITH_AES_256_CBC_SHA 0x0300C021
#define TLS1_CK_SRP_SHA_DSS_WITH_AES_256_CBC_SHA 0x0300C022

// ECDH HMAC based ciphersuites from RFC 5289

#define TLS1_CK_ECDHE_ECDSA_WITH_AES_128_SHA256 0x0300C023
#define TLS1_CK_ECDHE_ECDSA_WITH_AES_256_SHA384 0x0300C024
#define TLS1_CK_ECDH_ECDSA_WITH_AES_128_SHA256 0x0300C025
#define TLS1_CK_ECDH_ECDSA_WITH_AES_256_SHA384 0x0300C026
#define TLS1_CK_ECDHE_RSA_WITH_AES_128_SHA256 0x0300C027
#define TLS1_CK_ECDHE_RSA_WITH_AES_256_SHA384 0x0300C028
#define TLS1_CK_ECDH_RSA_WITH_AES_128_SHA256 0x0300C029
#define TLS1_CK_ECDH_RSA_WITH_AES_256_SHA384 0x0300C02A

// ECDH GCM based ciphersuites from RFC 5289
#define TLS1_CK_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 0x0300C02B
#define TLS1_CK_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384 0x0300C02C
#define TLS1_CK_ECDH_ECDSA_WITH_AES_128_GCM_SHA256 0x0300C02D
#define TLS1_CK_ECDH_ECDSA_WITH_AES_256_GCM_SHA384 0x0300C02E
#define TLS1_CK_ECDHE_RSA_WITH_AES_128_GCM_SHA256 0x0300C02F
#define TLS1_CK_ECDHE_RSA_WITH_AES_256_GCM_SHA384 0x0300C030
#define TLS1_CK_ECDH_RSA_WITH_AES_128_GCM_SHA256 0x0300C031
#define TLS1_CK_ECDH_RSA_WITH_AES_256_GCM_SHA384 0x0300C032

// ChaCha20-Poly1305 cipher suites from RFC 7905.
#define TLS1_CK_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256 0x0300CCA8
#define TLS1_CK_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256 0x0300CCA9
#define TLS1_CK_ECDHE_PSK_WITH_CHACHA20_POLY1305_SHA256 0x0300CCAC

// TLS 1.3 ciphersuites from RFC 8446.
#define TLS1_3_CK_AES_128_GCM_SHA256 0x03001301
#define TLS1_3_CK_AES_256_GCM_SHA384 0x03001302
#define TLS1_3_CK_CHACHA20_POLY1305_SHA256 0x03001303

// The following constants are legacy aliases of |TLS1_3_CK_*|.
// TODO(davidben): Migrate callers to the new name and remove these.
#define TLS1_CK_AES_128_GCM_SHA256 TLS1_3_CK_AES_128_GCM_SHA256
#define TLS1_CK_AES_256_GCM_SHA384 TLS1_3_CK_AES_256_GCM_SHA384
#define TLS1_CK_CHACHA20_POLY1305_SHA256 TLS1_3_CK_CHACHA20_POLY1305_SHA256

// XXX
// Inconsistency alert:
// The OpenSSL names of ciphers with ephemeral DH here include the string
// "DHE", while elsewhere it has always been "EDH".
// (The alias for the list of all such ciphers also is "EDH".)
// The specifications speak of "EDH"; maybe we should allow both forms
// for everything.
#define TLS1_TXT_RSA_EXPORT1024_WITH_RC4_56_MD5 "EXP1024-RC4-MD5"
#define TLS1_TXT_RSA_EXPORT1024_WITH_RC2_CBC_56_MD5 "EXP1024-RC2-CBC-MD5"
#define TLS1_TXT_RSA_EXPORT1024_WITH_DES_CBC_SHA "EXP1024-DES-CBC-SHA"
#define TLS1_TXT_DHE_DSS_EXPORT1024_WITH_DES_CBC_SHA \
  "EXP1024-DHE-DSS-DES-CBC-SHA"
#define TLS1_TXT_RSA_EXPORT1024_WITH_RC4_56_SHA "EXP1024-RC4-SHA"
#define TLS1_TXT_DHE_DSS_EXPORT1024_WITH_RC4_56_SHA "EXP1024-DHE-DSS-RC4-SHA"
#define TLS1_TXT_DHE_DSS_WITH_RC4_128_SHA "DHE-DSS-RC4-SHA"

// AES ciphersuites from RFC 3268
#define TLS1_TXT_RSA_WITH_AES_128_SHA "AES128-SHA"
#define TLS1_TXT_DH_DSS_WITH_AES_128_SHA "DH-DSS-AES128-SHA"
#define TLS1_TXT_DH_RSA_WITH_AES_128_SHA "DH-RSA-AES128-SHA"
#define TLS1_TXT_DHE_DSS_WITH_AES_128_SHA "DHE-DSS-AES128-SHA"
#define TLS1_TXT_DHE_RSA_WITH_AES_128_SHA "DHE-RSA-AES128-SHA"
#define TLS1_TXT_ADH_WITH_AES_128_SHA "ADH-AES128-SHA"

#define TLS1_TXT_RSA_WITH_AES_256_SHA "AES256-SHA"
#define TLS1_TXT_DH_DSS_WITH_AES_256_SHA "DH-DSS-AES256-SHA"
#define TLS1_TXT_DH_RSA_WITH_AES_256_SHA "DH-RSA-AES256-SHA"
#define TLS1_TXT_DHE_DSS_WITH_AES_256_SHA "DHE-DSS-AES256-SHA"
#define TLS1_TXT_DHE_RSA_WITH_AES_256_SHA "DHE-RSA-AES256-SHA"
#define TLS1_TXT_ADH_WITH_AES_256_SHA "ADH-AES256-SHA"

// ECC ciphersuites from RFC 4492
#define TLS1_TXT_ECDH_ECDSA_WITH_NULL_SHA "ECDH-ECDSA-NULL-SHA"
#define TLS1_TXT_ECDH_ECDSA_WITH_RC4_128_SHA "ECDH-ECDSA-RC4-SHA"
#define TLS1_TXT_ECDH_ECDSA_WITH_DES_192_CBC3_SHA "ECDH-ECDSA-DES-CBC3-SHA"
#define TLS1_TXT_ECDH_ECDSA_WITH_AES_128_CBC_SHA "ECDH-ECDSA-AES128-SHA"
#define TLS1_TXT_ECDH_ECDSA_WITH_AES_256_CBC_SHA "ECDH-ECDSA-AES256-SHA"

#define TLS1_TXT_ECDHE_ECDSA_WITH_NULL_SHA "ECDHE-ECDSA-NULL-SHA"
#define TLS1_TXT_ECDHE_ECDSA_WITH_RC4_128_SHA "ECDHE-ECDSA-RC4-SHA"
#define TLS1_TXT_ECDHE_ECDSA_WITH_DES_192_CBC3_SHA "ECDHE-ECDSA-DES-CBC3-SHA"
#define TLS1_TXT_ECDHE_ECDSA_WITH_AES_128_CBC_SHA "ECDHE-ECDSA-AES128-SHA"
#define TLS1_TXT_ECDHE_ECDSA_WITH_AES_256_CBC_SHA "ECDHE-ECDSA-AES256-SHA"

#define TLS1_TXT_ECDH_RSA_WITH_NULL_SHA "ECDH-RSA-NULL-SHA"
#define TLS1_TXT_ECDH_RSA_WITH_RC4_128_SHA "ECDH-RSA-RC4-SHA"
#define TLS1_TXT_ECDH_RSA_WITH_DES_192_CBC3_SHA "ECDH-RSA-DES-CBC3-SHA"
#define TLS1_TXT_ECDH_RSA_WITH_AES_128_CBC_SHA "ECDH-RSA-AES128-SHA"
#define TLS1_TXT_ECDH_RSA_WITH_AES_256_CBC_SHA "ECDH-RSA-AES256-SHA"

#define TLS1_TXT_ECDHE_RSA_WITH_NULL_SHA "ECDHE-RSA-NULL-SHA"
#define TLS1_TXT_ECDHE_RSA_WITH_RC4_128_SHA "ECDHE-RSA-RC4-SHA"
#define TLS1_TXT_ECDHE_RSA_WITH_DES_192_CBC3_SHA "ECDHE-RSA-DES-CBC3-SHA"
#define TLS1_TXT_ECDHE_RSA_WITH_AES_128_CBC_SHA "ECDHE-RSA-AES128-SHA"
#define TLS1_TXT_ECDHE_RSA_WITH_AES_256_CBC_SHA "ECDHE-RSA-AES256-SHA"

#define TLS1_TXT_ECDH_anon_WITH_NULL_SHA "AECDH-NULL-SHA"
#define TLS1_TXT_ECDH_anon_WITH_RC4_128_SHA "AECDH-RC4-SHA"
#define TLS1_TXT_ECDH_anon_WITH_DES_192_CBC3_SHA "AECDH-DES-CBC3-SHA"
#define TLS1_TXT_ECDH_anon_WITH_AES_128_CBC_SHA "AECDH-AES128-SHA"
#define TLS1_TXT_ECDH_anon_WITH_AES_256_CBC_SHA "AECDH-AES256-SHA"

// PSK ciphersuites from RFC 4279
#define TLS1_TXT_PSK_WITH_RC4_128_SHA "PSK-RC4-SHA"
#define TLS1_TXT_PSK_WITH_3DES_EDE_CBC_SHA "PSK-3DES-EDE-CBC-SHA"
#define TLS1_TXT_PSK_WITH_AES_128_CBC_SHA "PSK-AES128-CBC-SHA"
#define TLS1_TXT_PSK_WITH_AES_256_CBC_SHA "PSK-AES256-CBC-SHA"

// PSK ciphersuites from RFC 5489
#define TLS1_TXT_ECDHE_PSK_WITH_AES_128_CBC_SHA "ECDHE-PSK-AES128-CBC-SHA"
#define TLS1_TXT_ECDHE_PSK_WITH_AES_256_CBC_SHA "ECDHE-PSK-AES256-CBC-SHA"

// SRP ciphersuite from RFC 5054
#define TLS1_TXT_SRP_SHA_WITH_3DES_EDE_CBC_SHA "SRP-3DES-EDE-CBC-SHA"
#define TLS1_TXT_SRP_SHA_RSA_WITH_3DES_EDE_CBC_SHA "SRP-RSA-3DES-EDE-CBC-SHA"
#define TLS1_TXT_SRP_SHA_DSS_WITH_3DES_EDE_CBC_SHA "SRP-DSS-3DES-EDE-CBC-SHA"
#define TLS1_TXT_SRP_SHA_WITH_AES_128_CBC_SHA "SRP-AES-128-CBC-SHA"
#define TLS1_TXT_SRP_SHA_RSA_WITH_AES_128_CBC_SHA "SRP-RSA-AES-128-CBC-SHA"
#define TLS1_TXT_SRP_SHA_DSS_WITH_AES_128_CBC_SHA "SRP-DSS-AES-128-CBC-SHA"
#define TLS1_TXT_SRP_SHA_WITH_AES_256_CBC_SHA "SRP-AES-256-CBC-SHA"
#define TLS1_TXT_SRP_SHA_RSA_WITH_AES_256_CBC_SHA "SRP-RSA-AES-256-CBC-SHA"
#define TLS1_TXT_SRP_SHA_DSS_WITH_AES_256_CBC_SHA "SRP-DSS-AES-256-CBC-SHA"

// Camellia ciphersuites from RFC 4132
#define TLS1_TXT_RSA_WITH_CAMELLIA_128_CBC_SHA "CAMELLIA128-SHA"
#define TLS1_TXT_DH_DSS_WITH_CAMELLIA_128_CBC_SHA "DH-DSS-CAMELLIA128-SHA"
#define TLS1_TXT_DH_RSA_WITH_CAMELLIA_128_CBC_SHA "DH-RSA-CAMELLIA128-SHA"
#define TLS1_TXT_DHE_DSS_WITH_CAMELLIA_128_CBC_SHA "DHE-DSS-CAMELLIA128-SHA"
#define TLS1_TXT_DHE_RSA_WITH_CAMELLIA_128_CBC_SHA "DHE-RSA-CAMELLIA128-SHA"
#define TLS1_TXT_ADH_WITH_CAMELLIA_128_CBC_SHA "ADH-CAMELLIA128-SHA"

#define TLS1_TXT_RSA_WITH_CAMELLIA_256_CBC_SHA "CAMELLIA256-SHA"
#define TLS1_TXT_DH_DSS_WITH_CAMELLIA_256_CBC_SHA "DH-DSS-CAMELLIA256-SHA"
#define TLS1_TXT_DH_RSA_WITH_CAMELLIA_256_CBC_SHA "DH-RSA-CAMELLIA256-SHA"
#define TLS1_TXT_DHE_DSS_WITH_CAMELLIA_256_CBC_SHA "DHE-DSS-CAMELLIA256-SHA"
#define TLS1_TXT_DHE_RSA_WITH_CAMELLIA_256_CBC_SHA "DHE-RSA-CAMELLIA256-SHA"
#define TLS1_TXT_ADH_WITH_CAMELLIA_256_CBC_SHA "ADH-CAMELLIA256-SHA"

// SEED ciphersuites from RFC 4162
#define TLS1_TXT_RSA_WITH_SEED_SHA "SEED-SHA"
#define TLS1_TXT_DH_DSS_WITH_SEED_SHA "DH-DSS-SEED-SHA"
#define TLS1_TXT_DH_RSA_WITH_SEED_SHA "DH-RSA-SEED-SHA"
#define TLS1_TXT_DHE_DSS_WITH_SEED_SHA "DHE-DSS-SEED-SHA"
#define TLS1_TXT_DHE_RSA_WITH_SEED_SHA "DHE-RSA-SEED-SHA"
#define TLS1_TXT_ADH_WITH_SEED_SHA "ADH-SEED-SHA"

// TLS v1.2 ciphersuites
#define TLS1_TXT_RSA_WITH_NULL_SHA256 "NULL-SHA256"
#define TLS1_TXT_RSA_WITH_AES_128_SHA256 "AES128-SHA256"
#define TLS1_TXT_RSA_WITH_AES_256_SHA256 "AES256-SHA256"
#define TLS1_TXT_DH_DSS_WITH_AES_128_SHA256 "DH-DSS-AES128-SHA256"
#define TLS1_TXT_DH_RSA_WITH_AES_128_SHA256 "DH-RSA-AES128-SHA256"
#define TLS1_TXT_DHE_DSS_WITH_AES_128_SHA256 "DHE-DSS-AES128-SHA256"
#define TLS1_TXT_DHE_RSA_WITH_AES_128_SHA256 "DHE-RSA-AES128-SHA256"
#define TLS1_TXT_DH_DSS_WITH_AES_256_SHA256 "DH-DSS-AES256-SHA256"
#define TLS1_TXT_DH_RSA_WITH_AES_256_SHA256 "DH-RSA-AES256-SHA256"
#define TLS1_TXT_DHE_DSS_WITH_AES_256_SHA256 "DHE-DSS-AES256-SHA256"
#define TLS1_TXT_DHE_RSA_WITH_AES_256_SHA256 "DHE-RSA-AES256-SHA256"
#define TLS1_TXT_ADH_WITH_AES_128_SHA256 "ADH-AES128-SHA256"
#define TLS1_TXT_ADH_WITH_AES_256_SHA256 "ADH-AES256-SHA256"

// TLS v1.2 GCM ciphersuites from RFC 5288
#define TLS1_TXT_RSA_WITH_AES_128_GCM_SHA256 "AES128-GCM-SHA256"
#define TLS1_TXT_RSA_WITH_AES_256_GCM_SHA384 "AES256-GCM-SHA384"
#define TLS1_TXT_DHE_RSA_WITH_AES_128_GCM_SHA256 "DHE-RSA-AES128-GCM-SHA256"
#define TLS1_TXT_DHE_RSA_WITH_AES_256_GCM_SHA384 "DHE-RSA-AES256-GCM-SHA384"
#define TLS1_TXT_DH_RSA_WITH_AES_128_GCM_SHA256 "DH-RSA-AES128-GCM-SHA256"
#define TLS1_TXT_DH_RSA_WITH_AES_256_GCM_SHA384 "DH-RSA-AES256-GCM-SHA384"
#define TLS1_TXT_DHE_DSS_WITH_AES_128_GCM_SHA256 "DHE-DSS-AES128-GCM-SHA256"
#define TLS1_TXT_DHE_DSS_WITH_AES_256_GCM_SHA384 "DHE-DSS-AES256-GCM-SHA384"
#define TLS1_TXT_DH_DSS_WITH_AES_128_GCM_SHA256 "DH-DSS-AES128-GCM-SHA256"
#define TLS1_TXT_DH_DSS_WITH_AES_256_GCM_SHA384 "DH-DSS-AES256-GCM-SHA384"
#define TLS1_TXT_ADH_WITH_AES_128_GCM_SHA256 "ADH-AES128-GCM-SHA256"
#define TLS1_TXT_ADH_WITH_AES_256_GCM_SHA384 "ADH-AES256-GCM-SHA384"

// ECDH HMAC based ciphersuites from RFC 5289

#define TLS1_TXT_ECDHE_ECDSA_WITH_AES_128_SHA256 "ECDHE-ECDSA-AES128-SHA256"
#define TLS1_TXT_ECDHE_ECDSA_WITH_AES_256_SHA384 "ECDHE-ECDSA-AES256-SHA384"
#define TLS1_TXT_ECDH_ECDSA_WITH_AES_128_SHA256 "ECDH-ECDSA-AES128-SHA256"
#define TLS1_TXT_ECDH_ECDSA_WITH_AES_256_SHA384 "ECDH-ECDSA-AES256-SHA384"
#define TLS1_TXT_ECDHE_RSA_WITH_AES_128_SHA256 "ECDHE-RSA-AES128-SHA256"
#define TLS1_TXT_ECDHE_RSA_WITH_AES_256_SHA384 "ECDHE-RSA-AES256-SHA384"
#define TLS1_TXT_ECDH_RSA_WITH_AES_128_SHA256 "ECDH-RSA-AES128-SHA256"
#define TLS1_TXT_ECDH_RSA_WITH_AES_256_SHA384 "ECDH-RSA-AES256-SHA384"

// ECDH GCM based ciphersuites from RFC 5289
#define TLS1_TXT_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 \
  "ECDHE-ECDSA-AES128-GCM-SHA256"
#define TLS1_TXT_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384 \
  "ECDHE-ECDSA-AES256-GCM-SHA384"
#define TLS1_TXT_ECDH_ECDSA_WITH_AES_128_GCM_SHA256 \
  "ECDH-ECDSA-AES128-GCM-SHA256"
#define TLS1_TXT_ECDH_ECDSA_WITH_AES_256_GCM_SHA384 \
  "ECDH-ECDSA-AES256-GCM-SHA384"
#define TLS1_TXT_ECDHE_RSA_WITH_AES_128_GCM_SHA256 "ECDHE-RSA-AES128-GCM-SHA256"
#define TLS1_TXT_ECDHE_RSA_WITH_AES_256_GCM_SHA384 "ECDHE-RSA-AES256-GCM-SHA384"
#define TLS1_TXT_ECDH_RSA_WITH_AES_128_GCM_SHA256 "ECDH-RSA-AES128-GCM-SHA256"
#define TLS1_TXT_ECDH_RSA_WITH_AES_256_GCM_SHA384 "ECDH-RSA-AES256-GCM-SHA384"

#define TLS1_TXT_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256 \
  "ECDHE-RSA-CHACHA20-POLY1305"
#define TLS1_TXT_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256 \
  "ECDHE-ECDSA-CHACHA20-POLY1305"
#define TLS1_TXT_ECDHE_PSK_WITH_CHACHA20_POLY1305_SHA256 \
  "ECDHE-PSK-CHACHA20-POLY1305"

// TLS 1.3 ciphersuites from RFC 8446.
#define TLS1_3_RFC_AES_128_GCM_SHA256 "TLS_AES_128_GCM_SHA256"
#define TLS1_3_RFC_AES_256_GCM_SHA384 "TLS_AES_256_GCM_SHA384"
#define TLS1_3_RFC_CHACHA20_POLY1305_SHA256 "TLS_CHACHA20_POLY1305_SHA256"

// The following constants are legacy aliases of |TLS1_3_CK_*|.
// TODO(bbe): Migrate callers to the new name and remove these.
#define TLS1_TXT_AES_128_GCM_SHA256 TLS1_3_RFC_AES_128_GCM_SHA256
#define TLS1_TXT_AES_256_GCM_SHA384 TLS1_3_RFC_AES_256_GCM_SHA384
#define TLS1_TXT_CHACHA20_POLY1305_SHA256 TLS1_3_RFC_CHACHA20_POLY1305_SHA256

#define TLS_CT_RSA_SIGN 1
#define TLS_CT_DSS_SIGN 2
#define TLS_CT_RSA_FIXED_DH 3
#define TLS_CT_DSS_FIXED_DH 4
#define TLS_CT_ECDSA_SIGN 64
#define TLS_CT_RSA_FIXED_ECDH 65
#define TLS_CT_ECDSA_FIXED_ECDH 66

#define TLS_MD_MAX_CONST_SIZE 20


#ifdef  __cplusplus
}  // extern C
#endif

#endif  // OPENSSL_HEADER_TLS1_H
