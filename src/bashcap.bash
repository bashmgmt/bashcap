BASHCAP__IS_RUNNING=1

BASHCAP() {
    local IFS=' '
    local -a __bc_vars=()
    local -a __bc_notes=()
    while (( $# > 0 )); do
        case "$1" in
            -BCV:*) __bc_vars+=("${1#-BCV:}"); shift ;;
            -BCS:*) __bc_notes+=("${1#-BCS:}"); shift ;;
            *) break ;;
        esac
    done

    # BASH_ARGC[i] is meaningful only where it aligns 1:1 with FUNCNAME[i];
    # enabling `extdebug` part-way leaves it short, and short means every
    # width belongs to a different frame. Alignment is the test, not
    # `shopt -q`, and an unaligned one is carried as absent.
    local __bc_traced=no
    local -a __bc_argc=()
    if (( ${#BASH_ARGC[@]} == ${#FUNCNAME[@]} )); then
        __bc_traced=yes
        __bc_argc=("${BASH_ARGC[@]}")
    fi

    # BASH_ARGV is one flat stack: frame 0's arguments, then frame 1's, each
    # group reversed within itself. Summing the widths ahead of a group gives
    # where it starts — an index rather than a walk, and BASHCAP's own group
    # at index 0 falls behind every reported frame by construction.
    local -a __bc_from=()
    local -i __bc_i __bc_at=0
    for (( __bc_i = 0; __bc_i < ${#__bc_argc[@]}; __bc_i++ )); do
        __bc_from[__bc_i]=$__bc_at
        __bc_at=$(( __bc_at + __bc_argc[__bc_i] ))
    done

    # Frame 0 is BASHCAP's own; a frame's line is where the frame below made
    # the call, hence BASH_LINENO[i - 1].
    local -a __bc_frames=() __bc_frame=()
    local -i __bc_j
    for (( __bc_i = 1; __bc_i < ${#FUNCNAME[@]}; __bc_i++ )); do
        __bc_frame=(
            "${FUNCNAME[__bc_i]}"
            "${BASH_SOURCE[__bc_i]}"
            "${BASH_LINENO[__bc_i - 1]}"
        )

        # Counting down undoes the reversal, so arguments come out in the
        # order the call was written.
        for (( __bc_j = __bc_argc[__bc_i]; __bc_j > 0; __bc_j-- )); do
            __bc_frame+=("${BASH_ARGV[__bc_from[__bc_i] + __bc_j - 1]}")
        done

        __bc_frames+=("(${__bc_frame[*]@Q})")
    done

    local -a __bc_state=(
        subshell  "$BASH_SUBSHELL"
        shlvl     "$SHLVL"
        seconds   "$SECONDS"
        flags     "$-"
        bashopts  "$BASHOPTS"
        shellopts "$SHELLOPTS"
    )

    local -a __bc_rematch=()
    if [[ -n ${BASH_REMATCH[*]+set} ]]; then
        __bc_rematch=("${BASH_REMATCH[@]}")
    fi

    local -a __bc_declared=()
    local __bc_name
    for __bc_name in "${__bc_vars[@]}" ${!BASHCAP__CTX__@}; do
        declare -p "$__bc_name" &>/dev/null || continue
        local -n __bc_ref="$__bc_name"
        __bc_declared+=("${__bc_ref[*]@A}")
        unset -n __bc_ref
    done

    BC_INSTR say __BASHCAP__ \
        traced  "$__bc_traced" \
        frames  "(${__bc_frames[*]@Q})" \
        state   "(${__bc_state[*]@Q})" \
        rematch "(${__bc_rematch[*]@Q})" \
        vars    "(${__bc_declared[*]@Q})" \
        notes   "(${__bc_notes[*]@Q})"
}

WITH_BASHCAP() {
    local -a __bc_flags=()
    while (( $# > 0 )); do
        case "$1" in
            -BCV:*|-BCS:*) __bc_flags+=("$1"); shift ;;
            *) break ;;
        esac
    done

    # The guards the prelude defines are in scope here: this file is the
    # rig's bash, sourced after it. A snapshot that could not be taken is a
    # broken run, so it is forwarded rather than stepped over.
    BASHCAP "${__bc_flags[@]}" || __BC_BAIL

    "$@"
    local __bc_rc=$?
    return "$__bc_rc"
}
