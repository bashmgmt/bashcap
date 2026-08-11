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

    # Two frames are ours: __bc_stack's own and BASHCAP's.
    local -a __bc_walk=()
    __bc_stack __bc_walk 2

    local -a __bc_state=(
        subshell  "$BASH_SUBSHELL"
        shlvl     "$SHLVL"
        seconds   "$SECONDS"
        flags     "$-"
        bashopts  "$BASHOPTS"
        shellopts "$SHELLOPTS"
    )

    local -a __bc_rematch=("${BASH_REMATCH[@]}")

    local -a __bc_declared=()
    local __bc_name
    for __bc_name in "${__bc_vars[@]}" ${!BASHCAP__CTX__@}; do
        declare -p "$__bc_name" &>/dev/null || continue
        local -n __bc_ref="$__bc_name"
        __bc_declared+=("${__bc_ref[*]@A}")
        unset -n __bc_ref
    done

    BC_INSTR say __BASHCAP__ \
        "${__bc_walk[@]}" \
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
