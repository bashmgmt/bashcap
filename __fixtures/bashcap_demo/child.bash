#!/usr/bin/env bash
source "$(dirname "${BASH_SOURCE[0]}")/polyfill.bash"
child_work() {
    declare -a payload=(x y z)
    BASHCAP -BCV:payload -BCS:"child process, own pid and SHLVL"
}
child_work
