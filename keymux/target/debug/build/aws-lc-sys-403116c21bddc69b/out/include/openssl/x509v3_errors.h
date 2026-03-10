/* Written by Dr Stephen N Henson (steve@openssl.org) for the OpenSSL
 * project 1999. */
/* ====================================================================
 * Copyright (c) 1999-2004 The OpenSSL Project.  All rights reserved.
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
 *    for use in the OpenSSL Toolkit. (http://www.OpenSSL.org/)"
 *
 * 4. The names "OpenSSL Toolkit" and "OpenSSL Project" must not be used to
 *    endorse or promote products derived from this software without
 *    prior written permission. For written permission, please contact
 *    licensing@OpenSSL.org.
 *
 * 5. Products derived from this software may not be called "OpenSSL"
 *    nor may "OpenSSL" appear in their names without prior written
 *    permission of the OpenSSL Project.
 *
 * 6. Redistributions of any form whatsoever must retain the following
 *    acknowledgment:
 *    "This product includes software developed by the OpenSSL Project
 *    for use in the OpenSSL Toolkit (http://www.OpenSSL.org/)"
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
 * Hudson (tjh@cryptsoft.com). */

#ifndef OPENSSL_HEADER_X509V3_ERRORS_H
#define OPENSSL_HEADER_X509V3_ERRORS_H

#define X509V3_R_BAD_IP_ADDRESS 100
#define X509V3_R_BAD_OBJECT 101
#define X509V3_R_BN_DEC2BN_ERROR 102
#define X509V3_R_BN_TO_ASN1_INTEGER_ERROR 103
#define X509V3_R_CANNOT_FIND_FREE_FUNCTION 104
#define X509V3_R_DIRNAME_ERROR 105
#define X509V3_R_DISTPOINT_ALREADY_SET 106
#define X509V3_R_DUPLICATE_ZONE_ID 107
#define X509V3_R_ERROR_CONVERTING_ZONE 108
#define X509V3_R_ERROR_CREATING_EXTENSION 109
#define X509V3_R_ERROR_IN_EXTENSION 110
#define X509V3_R_EXPECTED_A_SECTION_NAME 111
#define X509V3_R_EXTENSION_EXISTS 112
#define X509V3_R_EXTENSION_NAME_ERROR 113
#define X509V3_R_EXTENSION_NOT_FOUND 114
#define X509V3_R_EXTENSION_SETTING_NOT_SUPPORTED 115
#define X509V3_R_EXTENSION_VALUE_ERROR 116
#define X509V3_R_ILLEGAL_EMPTY_EXTENSION 117
#define X509V3_R_ILLEGAL_HEX_DIGIT 118
#define X509V3_R_INCORRECT_POLICY_SYNTAX_TAG 119
#define X509V3_R_INVALID_BOOLEAN_STRING 120
#define X509V3_R_INVALID_EXTENSION_STRING 121
#define X509V3_R_INVALID_MULTIPLE_RDNS 122
#define X509V3_R_INVALID_NAME 123
#define X509V3_R_INVALID_NULL_ARGUMENT 124
#define X509V3_R_INVALID_NULL_NAME 125
#define X509V3_R_INVALID_NULL_VALUE 126
#define X509V3_R_INVALID_NUMBER 127
#define X509V3_R_INVALID_NUMBERS 128
#define X509V3_R_INVALID_OBJECT_IDENTIFIER 129
#define X509V3_R_INVALID_OPTION 130
#define X509V3_R_INVALID_POLICY_IDENTIFIER 131
#define X509V3_R_INVALID_PROXY_POLICY_SETTING 132
#define X509V3_R_INVALID_PURPOSE 133
#define X509V3_R_INVALID_SECTION 134
#define X509V3_R_INVALID_SYNTAX 135
#define X509V3_R_ISSUER_DECODE_ERROR 136
#define X509V3_R_MISSING_VALUE 137
#define X509V3_R_NEED_ORGANIZATION_AND_NUMBERS 138
#define X509V3_R_NO_CONFIG_DATABASE 139
#define X509V3_R_NO_ISSUER_CERTIFICATE 140
#define X509V3_R_NO_ISSUER_DETAILS 141
#define X509V3_R_NO_POLICY_IDENTIFIER 142
#define X509V3_R_NO_PROXY_CERT_POLICY_LANGUAGE_DEFINED 143
#define X509V3_R_NO_PUBLIC_KEY 144
#define X509V3_R_NO_SUBJECT_DETAILS 145
#define X509V3_R_ODD_NUMBER_OF_DIGITS 146
#define X509V3_R_OPERATION_NOT_DEFINED 147
#define X509V3_R_OTHERNAME_ERROR 148
#define X509V3_R_POLICY_LANGUAGE_ALREADY_DEFINED 149
#define X509V3_R_POLICY_PATH_LENGTH 150
#define X509V3_R_POLICY_PATH_LENGTH_ALREADY_DEFINED 151
#define X509V3_R_POLICY_WHEN_PROXY_LANGUAGE_REQUIRES_NO_POLICY 152
#define X509V3_R_SECTION_NOT_FOUND 153
#define X509V3_R_UNABLE_TO_GET_ISSUER_DETAILS 154
#define X509V3_R_UNABLE_TO_GET_ISSUER_KEYID 155
#define X509V3_R_UNKNOWN_BIT_STRING_ARGUMENT 156
#define X509V3_R_UNKNOWN_EXTENSION 157
#define X509V3_R_UNKNOWN_EXTENSION_NAME 158
#define X509V3_R_UNKNOWN_OPTION 159
#define X509V3_R_UNSUPPORTED_OPTION 160
#define X509V3_R_UNSUPPORTED_TYPE 161
#define X509V3_R_USER_TOO_LONG 162
#define X509V3_R_INVALID_VALUE 163
#define X509V3_R_TRAILING_DATA_IN_EXTENSION 164

#endif  // OPENSSL_HEADER_X509V3_ERRORS_H
