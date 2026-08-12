# Pass-through `BASHCAP` and `WITH_BASHCAP`, for a script whose call sites ship
# without the tool. Sourcing this installs nothing:
#
#     source lib/bashcap_polyfill.bash
#     declare -F BASHCAP >/dev/null || __define_bashcap_polyfill
#
# bashcap defines the real words through `BASH_ENV`, before the script's first
# line, so the guard is what leaves them in place. A function definition is
# global wherever it runs, so the guard may sit inside a function of your own.
__define_bashcap_polyfill() {
    BASHCAP() { true; }

    # The same leading flags the real word consumes, so a call site reads the
    # same with the tool and without it.
    WITH_BASHCAP() {
        while (( $# > 0 )); do
            case "$1" in
                -BCV:*|-BCS:*) shift ;;
                *) break ;;
            esac
        done
        "$@"
    }
}
