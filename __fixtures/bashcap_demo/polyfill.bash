# Source this in scripts that call BASHCAP or WITH_BASHCAP. Without bashcap
# they cost nothing; under it, the real definitions are already in place.
if [[ -z "${BASHCAP__IS_RUNNING:-}" ]]; then
    WITH_BASHCAP() { "$@"; }
    BASHCAP() { true; }
fi
