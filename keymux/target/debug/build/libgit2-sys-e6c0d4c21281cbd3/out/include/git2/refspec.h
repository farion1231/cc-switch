/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_refspec_h__
#define INCLUDE_git_refspec_h__

#include "common.h"
#include "types.h"
#include "net.h"
#include "buffer.h"

/**
 * @file git2/refspec.h
 * @brief Git refspec attributes
 * @defgroup git_refspec Git refspec attributes
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Parse a given refspec string
 *
 * @param refspec a pointer to hold the refspec handle
 * @param input the refspec string
 * @param is_fetch is this a refspec for a fetch
 * @return 0 if the refspec string could be parsed, -1 otherwise
 */
GIT_EXTERN(int) git_refspec_parse(git_refspec **refspec, const char *input, int is_fetch);

/**
 * Free a refspec object which has been created by git_refspec_parse
 *
 * @param refspec the refspec object
 */
GIT_EXTERN(void) git_refspec_free(git_refspec *refspec);

/**
 * Get the source specifier
 *
 * @param refspec the refspec
 * @return the refspec's source specifier
 */
GIT_EXTERN(const char *) git_refspec_src(const git_refspec *refspec);

/**
 * Get the destination specifier
 *
 * @param refspec the refspec
 * @return the refspec's destination specifier
 */
GIT_EXTERN(const char *) git_refspec_dst(const git_refspec *refspec);

/**
 * Get the refspec's string
 *
 * @param refspec the refspec
 * @returns the refspec's original string
 */
GIT_EXTERN(const char *) git_refspec_string(const git_refspec *refspec);

/**
 * Get the force update setting
 *
 * @param refspec the refspec
 * @return 1 if force update has been set, 0 otherwise
 */
GIT_EXTERN(int) git_refspec_force(const git_refspec *refspec);

/**
 * Get the refspec's direction.
 *
 * @param spec refspec
 * @return GIT_DIRECTION_FETCH or GIT_DIRECTION_PUSH
 */
GIT_EXTERN(git_direction) git_refspec_direction(const git_refspec *spec);

/**
 * Check if a refspec's source descriptor matches a reference 
 *
 * @param refspec the refspec
 * @param refname the name of the reference to check
 * @return 1 if the refspec matches, 0 otherwise
 */
GIT_EXTERN(int) git_refspec_src_matches(const git_refspec *refspec, const char *refname);

/**
 * Check if a refspec's destination descriptor matches a reference
 *
 * @param refspec the refspec
 * @param refname the name of the reference to check
 * @return 1 if the refspec matches, 0 otherwise
 */
GIT_EXTERN(int) git_refspec_dst_matches(const git_refspec *refspec, const char *refname);

/**
 * Transform a reference to its target following the refspec's rules
 *
 * @param out where to store the target name
 * @param spec the refspec
 * @param name the name of the reference to transform
 * @return 0, GIT_EBUFS or another error
 */
GIT_EXTERN(int) git_refspec_transform(git_buf *out, const git_refspec *spec, const char *name);

/**
 * Transform a target reference to its source reference following the refspec's rules
 *
 * @param out where to store the source reference name
 * @param spec the refspec
 * @param name the name of the reference to transform
 * @return 0, GIT_EBUFS or another error
 */
GIT_EXTERN(int) git_refspec_rtransform(git_buf *out, const git_refspec *spec, const char *name);

GIT_END_DECL

#endif
