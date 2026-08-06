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

    # Bash records call arguments only under `extdebug`, which this never
    # turns on: from BASH_ENV that means "start the debugger", and it makes
    # ERR and DEBUG traps inherited. A subject already tracing gets them for
    # free. Equal lengths is the test, not `shopt -q`: enabling extdebug
    # part-way leaves BASH_ARGC short, and short means misattributed.
    local __bc_traced=no
    (( ${#BASH_ARGC[@]} == ${#FUNCNAME[@]} )) && __bc_traced=yes

    local -a __bc_frames=()
    local -i __bc_i __bc_argc __bc_j
    local -i __bc_at=${BASH_ARGC[0]:-0}     # step over BASHCAP's own flags
    for (( __bc_i = 1; __bc_i < ${#FUNCNAME[@]}; __bc_i++ )); do
        local -a __bc_frame=(
            "${FUNCNAME[__bc_i]}"
            "${BASH_SOURCE[__bc_i]}"
            "${BASH_LINENO[__bc_i - 1]}"
        )

        if [[ $__bc_traced == yes ]]; then
            # BASH_ARGV is one flat stack, innermost frame first, and each
            # frame's own arguments run backwards within it.
            __bc_argc=${BASH_ARGC[__bc_i]}
            for (( __bc_j = __bc_argc; __bc_j > 0; __bc_j-- )); do
                __bc_frame+=("${BASH_ARGV[__bc_at + __bc_j - 1]}")
            done
            (( __bc_at += __bc_argc ))
        fi

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
    if [[ -n ${BASH_REMATCH[@]+set} ]]; then
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

    BASHCAP "${__bc_flags[@]}"
    "$@"
    local __bc_rc=$?
    return "$__bc_rc"
}
