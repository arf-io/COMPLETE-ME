#!/bin/bash
usage() {
  cat <<'USAGE'
tool — does things
  tool push <bundle.tar>   upload a bundle
  tool status              show what is live
USAGE
}
while getopts "vf:" opt; do :; done
case "$1" in
  push) shift ;;
  status) ;;
  rollback) ;;   # go back a release
  -q|--quiet) ;;
  *) usage ;;
esac
