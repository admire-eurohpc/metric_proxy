#!/bin/sh -e
#
# Copyright (c) 2015-2023 The strace developers.
# All rights reserved.
#
# SPDX-License-Identifier: LGPL-2.1-or-later

[ "x${D:-0}" != x1 ] || set -x

cd "$(dirname "$0")"

list="$(sed -E -n '/^strace_SOURCES[[:space:]]*=/,/^[[:space:]]*# end of strace_SOURCES/ s/^[[:space:]]*([[:alnum:]][^.]*\.c)[[:space:]]*\\$/\1/p' Makefile.am |
	xargs -r grep -Elx '#[[:space:]]*include[[:space:]]+MPERS_DEFS' |
	tr '\n' ' ')"

[ -n "$list" ] || {
	echo >&2 "$0: error: the list of mpers source files is empty"
	exit 1
}

cat > mpers.am <<EOF
# Generated by $0; do not edit.
mpers_source_files = $list
EOF

sed -E -n 's/^#[[:space:]]*include[[:space:]]*"xlat\/([a-z][a-z_0-9]*)\.h".*/extern const struct xlat \1[];/p' \
	$list > mpers_xlat.h