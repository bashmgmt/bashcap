# The words bashcap gives a script. What a call does lives behind
# __bc_capture, whose argument is how many leading frames of the walk belong
# to the instrument — a number each word knows about itself.

# Leading -BCV:/-BCS: flags off the front, into two arrays of the caller's.
# Every consumed word lands in one of them, so their combined length is how far
# the caller has to shift.
__bc_take_flags() {
    local -n __bc_v="$1" __bc_n="$2"
    shift 2

    while (( $# > 0 )); do
        case "$1" in
            -BCV:*) __bc_v+=("${1#-BCV:}"); shift ;;
            -BCS:*) __bc_n+=("${1#-BCS:}"); shift ;;
            *) break ;;
        esac
    done
}

BASHCAP() {
    local -a __bc_vars=() __bc_notes=()
    __bc_take_flags __bc_vars __bc_notes "$@"

    __bc_capture 3
}

WITH_BASHCAP() {
    local -a __bc_vars=() __bc_notes=()
    __bc_take_flags __bc_vars __bc_notes "$@"
    shift $(( ${#__bc_vars[@]} + ${#__bc_notes[@]} ))

    # A snapshot that could not be taken is a broken run, so it is forwarded
    # rather than stepped over.
    __bc_capture 2 || return $?

    "$@"
}

# What a call does. $1 is how many leading frames of the walk belong to the
# instrument, counting `__bc_stack`'s own and this one; the word that called
# knows its own depth.
#
# The flags the word parsed are read here as the caller's locals, which is
# what dynamic scoping is for. So is `IFS`: it is taken for this frame alone, since
# every join below is `[*]@Q` and uses the caller's, and a subject with one of
# its own would corrupt them. Returning gives the subject's back — including an
# `IFS` that was unset — before anything of the subject's runs.
# ANCHOR: capture
__bc_capture() {
    local IFS=' '

    local -a __bc_walk=()
    __bc_stack __bc_walk "$1"

    # What changes while a shell runs and nothing else says. The rest of what a
    # shell is — which bash, how it was started, which options it had on, how
    # deep a subshell it is — it said once when it joined.
    local -a __bc_state=(
        seconds "$SECONDS"
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

    BC_INSTR BASHCAP say __BASHCAP__ \
        "${__bc_walk[@]}" \
        state   "(${__bc_state[*]@Q})" \
        rematch "(${__bc_rematch[*]@Q})" \
        vars    "(${__bc_declared[*]@Q})" \
        notes   "(${__bc_notes[*]@Q})"
}
# ANCHOR_END: capture
