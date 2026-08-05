if [[ -z "${BASHCAP__IS_RUNNING:-}" ]]; then
    WITH_BASHCAP() { "$@"; }
    BASHCAP() { true; }
fi
