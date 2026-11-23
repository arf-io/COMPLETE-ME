# COMPLETE-ME
.PHONY: install install-yes uninstall test build check

install:        ## interactive install: deps, shell hooks, self-test
	./install.sh

install-yes:    ## non-interactive install, yes to everything
	./install.sh --yes

uninstall:
	./install.sh --uninstall

test:
	cargo test --locked --release

build:
	cargo build --locked --release

check:          ## syntax-check the shell parts
	for f in install.sh shell/bash/complete_me.bash tests/harness/bash_capture.bash; do bash -n $$f || exit 1; done
	for f in shell/fish/complete_me.fish shell/fish/completions/*.fish; do fish -n $$f || exit 1; done
	for f in shell/zsh/complete_me.zsh tests/harness/zsh_capture.zsh; do zsh -n $$f || exit 1; done
